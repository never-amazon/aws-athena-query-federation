use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use indexmap::IndexMap;

use athena_federation_sdk::data::{BlockAllocator, BlockSpiller, SimpleBlockWriter};
use athena_federation_sdk::handlers::{
    CompositeHandler, HandlerContext, MetadataContext, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};

use super::metadata::MetadataDelegate;
use super::record::{ConnectionSource, JdbcRecordHandler, RecordDelegate, RecordDialect};
use crate::connection::owned;
use crate::connection::{
    DatabaseConnectionConfig, DatabaseConnectionConfigBuilder, JdbcConnectionFactory,
    CONNECTION_STRING_PROPERTY_SUFFIX, DEFAULT_CONNECTION_STRING_PROPERTY,
};
use crate::query::SplitQuery;
use crate::sql::{JdbcConnection, PreparedStatement};
use crate::validate::{not_blank, null_pointer};

const MAX_CATALOGS_TO_MULTIPLEX: usize = 100;
const DEFAULT_CATALOG_PREFIX: &str = "lambda:";
const LAMBDA_FUNCTION_NAME_PROPERTY: &str = "AWS_LAMBDA_FUNCTION_NAME";

pub const CATALOG_NOT_REGISTERED_ERROR_TEMPLATE: &str = "Catalog is not supported in multiplexer. After registering the catalog in Athena, must set '%s_connection_string' environment variable in Lambda. See JDBC connector README for further details.";

pub(crate) fn catalog_not_registered(catalog_name: &str) -> FederationError {
    FederationError::invalid_input(
        CATALOG_NOT_REGISTERED_ERROR_TEMPLATE.replace("%s", catalog_name),
    )
}

pub(crate) fn validate_handler_map<V>(map: &IndexMap<String, V>, message: &str) -> Result<()> {
    if map.is_empty() {
        return Err(FederationError::IllegalArgument(message.to_string()));
    }
    Ok(())
}

pub(crate) fn validate_catalog_count<V>(map: &IndexMap<String, V>) -> Result<()> {
    if map.len() > MAX_CATALOGS_TO_MULTIPLEX {
        return Err(FederationError::invalid_input(
            "Max 100 catalogs supported in multiplexer.",
        ));
    }
    Ok(())
}

pub(crate) async fn close_all<'a, F: std::future::Future<Output = Result<()>>>(
    kind: &str,
    closes: impl IntoIterator<Item = (&'a String, F)>,
) -> Option<FederationError> {
    let mut first_failure = None;
    for (catalog, close) in closes {
        if let Err(e) = close.await {
            tracing::warn!(
                "Failed to close {kind} handler for catalog {} {}",
                catalog,
                e
            );
            first_failure.get_or_insert(e);
        }
    }
    first_failure
}

pub struct MultiplexingJdbcMetadataHandler<H> {
    context: MetadataContext,
    connection_factory: Option<Arc<dyn JdbcConnectionFactory>>,
    delegates: IndexMap<String, Arc<H>>,
}

impl<H: MetadataHandler + MetadataDelegate + 'static> MultiplexingJdbcMetadataHandler<H> {
    pub fn new(
        context: MetadataContext,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
        delegates: IndexMap<String, Arc<H>>,
    ) -> Result<MultiplexingJdbcMetadataHandler<H>> {
        validate_handler_map(&delegates, "metadataHandlerMap must not be empty")?;
        validate_catalog_count(&delegates)?;
        Ok(MultiplexingJdbcMetadataHandler {
            context,
            connection_factory: Some(connection_factory),
            delegates,
        })
    }

    pub fn from_config(
        context: MetadataContext,
        engine: &str,
        config_options: &HashMap<String, String>,
        make_delegate: impl Fn(&DatabaseConnectionConfig) -> Result<Arc<H>>,
    ) -> Result<MultiplexingJdbcMetadataHandler<H>> {
        let delegates = handler_map(engine, config_options, make_delegate)?;
        validate_handler_map(&delegates, "Could not find any delegatee.")?;
        Ok(MultiplexingJdbcMetadataHandler {
            context,
            connection_factory: None,
            delegates,
        })
    }

    pub fn delegates(&self) -> &IndexMap<String, Arc<H>> {
        &self.delegates
    }

    fn delegate(&self, catalog_name: &str) -> Result<&H> {
        self.delegates
            .get(catalog_name)
            .map(AsRef::as_ref)
            .ok_or_else(|| catalog_not_registered(catalog_name))
    }

    pub fn partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        self.delegate(catalog_name)?.partition_schema(catalog_name)
    }

    pub async fn close(&self) -> Result<()> {
        let mut first_failure = close_all(
            "metadata",
            self.delegates.iter().map(|(c, d)| (c, d.close())),
        )
        .await;
        if let Some(factory) = &self.connection_factory {
            if let Err(e) = factory.close().await {
                first_failure.get_or_insert(e);
            }
        }
        first_failure.map_or(Ok(()), Err)
    }
}

#[async_trait]
impl<H: MetadataHandler + MetadataDelegate + 'static> MetadataHandler
    for MultiplexingJdbcMetadataHandler<H>
{
    fn context(&self) -> &MetadataContext {
        &self.context
    }

    async fn list_schema_names(&self, request: &ListSchemasRequest) -> Result<ListSchemasResponse> {
        self.delegate(request.catalog_name())?
            .list_schema_names(request)
            .await
    }

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse> {
        self.delegate(request.catalog_name())?
            .list_tables(request)
            .await
    }

    async fn get_query_passthrough_schema(
        &self,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.delegate(request.catalog_name())?
            .get_query_passthrough_schema(request)
            .await
    }

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse> {
        self.delegate(request.catalog_name())?
            .get_table(request)
            .await
    }

    async fn get_table_layout(
        &self,
        allocator: &dyn BlockAllocator,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        self.delegate(request.catalog_name())?
            .get_table_layout(allocator, request)
            .await
    }

    async fn get_partitions(
        &self,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.delegate(request.catalog_name())?
            .get_partitions(block_writer, request, query_status_checker)
            .await
    }

    async fn get_splits(&self, request: &GetSplitsRequest) -> Result<GetSplitsResponse> {
        self.delegate(request.catalog_name())?
            .get_splits(request)
            .await
    }

    async fn get_data_source_capabilities(
        &self,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.delegate(request.catalog_name())?
            .get_data_source_capabilities(request)
            .await
    }
}

pub type MultiplexingJdbcRecordHandler<H> = JdbcRecordHandler<RecordMultiplexer<H>>;

pub struct RecordMultiplexer<H> {
    delegates: IndexMap<String, Arc<H>>,
}

impl<H: RecordHandler + RecordDelegate + 'static> RecordMultiplexer<H> {
    pub fn handler(
        context: HandlerContext,
        connection_config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
        delegates: IndexMap<String, Arc<H>>,
    ) -> Result<MultiplexingJdbcRecordHandler<H>> {
        validate_handler_map(&delegates, "recordHandlerMap must not be empty")?;
        validate_catalog_count(&delegates)?;
        Ok(JdbcRecordHandler::new(
            context,
            ConnectionSource::Direct {
                config: connection_config,
                factory: connection_factory,
            },
            RecordMultiplexer { delegates },
        ))
    }

    pub fn handler_from_config(
        context: HandlerContext,
        engine: &str,
        config_options: &HashMap<String, String>,
        make_delegate: impl Fn(&DatabaseConnectionConfig) -> Result<Arc<H>>,
    ) -> Result<MultiplexingJdbcRecordHandler<H>> {
        let delegates = handler_map(engine, config_options, make_delegate)?;
        validate_handler_map(&delegates, "Could not find any delegatee.")?;
        Ok(JdbcRecordHandler::new(
            context,
            ConnectionSource::Delegating,
            RecordMultiplexer { delegates },
        ))
    }

    pub fn delegates(&self) -> &IndexMap<String, Arc<H>> {
        &self.delegates
    }
}

#[async_trait]
impl<H: RecordHandler + RecordDelegate + 'static> RecordDialect for RecordMultiplexer<H> {
    async fn build_split_sql(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        connection: &dyn JdbcConnection,
        query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        let delegate = self.delegates.get(query.catalog_name).ok_or_else(|| {
            null_pointer("Cannot invoke \"com.amazonaws.athena.connectors.jdbc.manager.JdbcRecordHandler.buildSplitSql(java.sql.Connection, String, com.amazonaws.athena.connector.lambda.domain.TableName, org.apache.arrow.vector.types.pojo.Schema, com.amazonaws.athena.connector.lambda.domain.predicate.Constraints, com.amazonaws.athena.connector.lambda.domain.Split)\" because the return value of \"java.util.Map.get(Object)\" is null")
        })?;
        RecordDelegate::build_split_sql(delegate.as_ref(), connection, query).await
    }

    async fn read_with_constraint(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.delegates
            .get(request.catalog_name())
            .ok_or_else(|| catalog_not_registered(request.catalog_name()))?
            .read_with_constraint(spiller, request, query_status_checker)
            .await
    }

    async fn close(&self) -> Result<()> {
        let closes: Vec<_> = self
            .delegates
            .iter()
            .map(|(c, d)| (c, RecordDelegate::close(d.as_ref())))
            .collect();
        close_all("record", closes).await.map_or(Ok(()), Err)
    }
}

pub fn has_catalog_connections<S>(environment: &HashMap<String, String, S>) -> bool {
    environment
        .keys()
        .any(|k| k.ends_with(CONNECTION_STRING_PROPERTY_SUFFIX))
}

pub async fn multiplexing_composite_handler<S, M, R>(
    environment: HashMap<String, String, S>,
    metadata_handler: M,
    record_handler: R,
) -> Result<CompositeHandler>
where
    S: Clone,
    M: AsyncFnOnce(bool, HashMap<String, String, S>) -> Result<Arc<dyn MetadataHandler>>,
    R: AsyncFnOnce(bool, HashMap<String, String, S>) -> Result<Arc<dyn RecordHandler>>,
{
    let multiplexed = has_catalog_connections(&environment);
    let metadata = metadata_handler(multiplexed, environment.clone()).await?;
    let record = record_handler(multiplexed, environment).await?;
    Ok(CompositeHandler::new(metadata, record, None))
}

fn insert_unique<V>(map: &mut IndexMap<String, V>, key: String, value: V) -> Result<()> {
    if map.contains_key(&key) {
        return Err(FederationError::IllegalArgument(format!(
            "Multiple entries with same key: {key}"
        )));
    }
    map.insert(key, value);
    Ok(())
}

fn connection_configs<S: BuildHasher>(
    engine: &str,
    config_options: &HashMap<String, String, S>,
) -> Result<(String, Vec<DatabaseConnectionConfig>)> {
    let function_name = not_blank(
        config_options
            .get(LAMBDA_FUNCTION_NAME_PROPERTY)
            .map(String::as_str),
        "Lambda function name not present in environment.",
    )?
    .to_string();
    let configs = DatabaseConnectionConfigBuilder::new()
        .engine(engine)
        .properties(owned(config_options))
        .build()?;
    if configs.is_empty() {
        return Err(FederationError::invalid_input(
            "At least one connection string required.",
        ));
    }
    Ok((function_name, configs))
}

fn missing_default() -> FederationError {
    FederationError::invalid_input(format!(
        "Must provide connection parameters for default database instance {DEFAULT_CONNECTION_STRING_PROPERTY}"
    ))
}

pub fn handler_map<H: ?Sized, S: BuildHasher>(
    engine: &str,
    config_options: &HashMap<String, String, S>,
    make_handler: impl Fn(&DatabaseConnectionConfig) -> Result<Arc<H>>,
) -> Result<IndexMap<String, Arc<H>>> {
    let (function_name, configs) = connection_configs(engine, config_options)?;
    let mut handlers: IndexMap<String, Arc<H>> = IndexMap::new();
    let mut default_present = false;
    for config in &configs {
        let handler = make_handler(config)?;
        insert_unique(
            &mut handlers,
            config.catalog().to_string(),
            Arc::clone(&handler),
        )?;
        if config.catalog() == DEFAULT_CONNECTION_STRING_PROPERTY {
            insert_unique(
                &mut handlers,
                format!("{DEFAULT_CATALOG_PREFIX}{function_name}"),
                handler,
            )?;
            default_present = true;
        }
    }
    if !default_present {
        return Err(missing_default());
    }
    Ok(handlers)
}
