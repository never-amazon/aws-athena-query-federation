use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use indexmap::IndexMap;

use athena_federation_sdk::data::{BlockAllocator, SimpleBlockWriter};
use athena_federation_sdk::handlers::{MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};

use crate::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use crate::manager::{jdbc_util, MetadataDelegate};

const MAX_CATALOGS_TO_MULTIPLEX: usize = 100;

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
        let delegates = jdbc_util::handler_map(engine, config_options, make_delegate)?;
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
