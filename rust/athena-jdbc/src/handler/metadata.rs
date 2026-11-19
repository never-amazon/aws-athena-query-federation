use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};

use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::{
    minor_type_of, ArrowType, FieldBuilder, SchemaBuilder, SimpleBlockWriter,
};
use athena_federation_sdk::domain::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{HandlerContext, MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
    UNLIMITED_PAGE_SIZE_VALUE,
};
use athena_federation_sdk::security::{AwsRequestOverrideConfiguration, FederatedIdentity};
use athena_federation_sdk::util::manual_pagination;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};

use super::record::{missing_factory, ConnectionSource};
use crate::arrow_type::jdbc_to_arrow_type;
use crate::java_null::OrJavaNull;
use crate::query::passthrough::{JDBC_QUERY_PASSTHROUGH, QUERY};
use crate::resolver::JdbcCaseResolver;
use crate::sql::{
    ColumnInfo, ColumnMetadata, DatabaseMetaData, JdbcConnection, PreparedStatement, Row, SqlError,
};
use crate::validate::close_after;

pub const TABLES_AND_VIEWS: &str = "Tables and Views";
const TABLE_TYPES: [&str; 4] = ["TABLE", "VIEW", "EXTERNAL TABLE", "MATERIALIZED VIEW"];

#[async_trait]
pub trait MetadataDialect: Send + Sync + Sized + 'static {
    fn partition_schema(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        catalog_name: &str,
    ) -> Result<Schema>;

    async fn get_partitions(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()>;

    async fn get_splits(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse>;

    async fn list_database_names(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        connection: &dyn JdbcConnection,
    ) -> Result<Vec<String>> {
        list_database_names(connection).await
    }

    async fn list_schema_tables(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        handler.list_tables_by_type(connection, database_name).await
    }

    async fn list_paginated_tables(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        connection: &dyn JdbcConnection,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        handler.paginate_tables(connection, request).await
    }

    async fn get_schema(
        &self,
        handler: &JdbcMetadataHandler<Self>,
        connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        handler
            .schema_from_columns(connection, table_name, partition_schema)
            .await
    }

    fn convert_datasource_type(
        &self,
        column: &ColumnMetadata,
        config_options: &HashMap<String, String>,
    ) -> Result<Option<ArrowType>> {
        jdbc_to_arrow_type(
            column.jdbc_type(),
            column.precision(),
            column.scale(),
            config_options,
        )
    }

    fn array_element_type(
        &self,
        _type_name: Option<&str>,
        _precision: i32,
        _scale: i32,
    ) -> ArrowType {
        DataType::Utf8
    }

    fn enhance_partition_schema(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        _partition_schema_builder: &mut SchemaBuilder,
        _request: &GetTableLayoutRequest,
    ) {
    }

    fn capabilities(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            IndexMap::new(),
        ))
    }

    async fn credentials_provider(
        &self,
        context: &HandlerContext,
        secret_name: Option<&str>,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        context
            .credentials_provider(secret_name, override_config)
            .await
    }
}

pub struct JdbcMetadataHandler<D> {
    context: MetadataContext,
    connection: ConnectionSource,
    case_resolver: JdbcCaseResolver,
    dialect: D,
}

impl<D: MetadataDialect> JdbcMetadataHandler<D> {
    pub fn new(
        context: MetadataContext,
        connection: ConnectionSource,
        case_resolver: JdbcCaseResolver,
        dialect: D,
    ) -> JdbcMetadataHandler<D> {
        JdbcMetadataHandler {
            context,
            connection,
            case_resolver,
            dialect,
        }
    }

    pub fn dialect(&self) -> &D {
        &self.dialect
    }

    pub fn connection(&self) -> &ConnectionSource {
        &self.connection
    }

    pub fn case_resolver(&self) -> &JdbcCaseResolver {
        &self.case_resolver
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        self.context.config_options()
    }

    pub async fn connect(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let ConnectionSource::Direct { config, factory } = &self.connection else {
            return Err(missing_factory());
        };
        let override_config = self
            .context
            .handler()
            .identity_override_config(identity)
            .await?;
        let credentials = self
            .dialect
            .credentials_provider(
                self.context.handler(),
                config.secret(),
                override_config.as_ref(),
            )
            .await?;
        factory.get_connection(credentials.as_deref()).await
    }

    pub async fn close(&self) -> Result<()> {
        match &self.connection {
            ConnectionSource::Direct { factory, .. } => factory.close().await,
            ConnectionSource::Delegating => Ok(()),
        }
    }

    pub fn partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        self.dialect.partition_schema(self, catalog_name)
    }

    pub async fn list_database_names(
        &self,
        connection: &dyn JdbcConnection,
    ) -> Result<Vec<String>> {
        self.dialect.list_database_names(self, connection).await
    }

    pub async fn list_schema_tables(
        &self,
        connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        self.dialect
            .list_schema_tables(self, connection, database_name)
            .await
    }

    pub async fn list_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.dialect
            .list_paginated_tables(self, connection, request)
            .await
    }

    pub async fn schema(
        &self,
        connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        self.dialect
            .get_schema(self, connection, table_name, partition_schema)
            .await
    }

    async fn adjusted_schema_name(
        &self,
        connection: &dyn JdbcConnection,
        schema_name: Option<&str>,
    ) -> Result<String> {
        self.case_resolver
            .get_adjusted_schema_name_string(
                connection,
                schema_name.unwrap_or_default(),
                self.config_options(),
            )
            .await
    }

    pub async fn paginate_tables(
        &self,
        connection: &dyn JdbcConnection,
        list_tables_request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        let adjusted_schema_name = self
            .adjusted_schema_name(connection, list_tables_request.schema_name())
            .await?;
        tracing::debug!(
            "Request is asking for pagination, but true pagination has not been implemented."
        );
        let page_size = list_tables_request.page_size();
        let start_token = list_tables_request.next_token();
        let all_tables = self
            .list_schema_tables(connection, &adjusted_schema_name)
            .await?;
        let (catalog_name, tables, next_token) = manual_pagination(
            &all_tables,
            start_token,
            page_size,
            list_tables_request.catalog_name(),
        )?
        .into_parts();
        Ok(ListTablesResponse::new(catalog_name, tables, next_token))
    }

    pub async fn list_tables_by_type(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        tracing::debug!("listTables, schemaName:{}", database_name);
        let metadata = jdbc_connection.metadata();
        let escape = metadata.search_string_escape();
        let catalog = jdbc_connection.get_catalog().await?;
        let schema_pattern = escape_name_pattern(Some(database_name), escape.as_deref())?;
        Ok(metadata
            .tables(
                catalog.as_deref(),
                schema_pattern.as_deref(),
                None,
                Some(&TABLE_TYPES),
            )
            .await?
            .into_iter()
            .map(|table| TableName::new(table.schema.unwrap_or_default(), table.name))
            .collect())
    }

    pub async fn schema_from_columns(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        let config_options = self.config_options();
        let mut schema_builder = SchemaBuilder::new_builder();
        let catalog = jdbc_connection.get_catalog().await?;
        let columns = columns(catalog.as_deref(), table_name, jdbc_connection.metadata()).await?;
        let found = !columns.is_empty();
        for column in columns {
            let column_size = column.column_size.unwrap_or(0);
            let decimal_digits = column.decimal_digits.unwrap_or(0);
            let column_type = jdbc_to_arrow_type(
                column.data_type,
                column_size,
                decimal_digits,
                config_options,
            )?;
            let column_name = column.name.or_java_null();
            match column_type {
                Some(arrow_type) if minor_type_of(&arrow_type).is_supported() => {
                    if matches!(arrow_type, DataType::List(_)) {
                        schema_builder.add_list_field(
                            &column_name,
                            self.dialect.array_element_type(
                                column.type_name.as_deref(),
                                column_size,
                                decimal_digits,
                            ),
                        );
                    } else {
                        schema_builder
                            .add_field(FieldBuilder::new_builder(&column_name, arrow_type).build());
                    }
                }
                other => {
                    tracing::warn!(
                        "getSchema: Unable to map type for column[{}] to a supported type, attempted {:?} - defaulting type to VARCHAR.",
                        column_name,
                        other
                    );
                    schema_builder
                        .add_field(FieldBuilder::new_builder(&column_name, DataType::Utf8).build());
                }
            }
        }

        if found {
            for field in partition_schema.fields() {
                schema_builder.add_field(Arc::clone(field));
            }
        } else {
            tracing::warn!(
                "getSchema: Could not find table {} in {}",
                table_name.table_name(),
                table_name.schema_name()
            );
        }
        Ok(schema_builder.build())
    }

    pub async fn query_passthrough_split(
        &self,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        let builder = self.context.split_builder(request).await?;
        let qpt_arguments: HashMap<String, String> = request
            .constraints()
            .query_passthrough_arguments()
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
            .collect();
        let split = builder.apply_properties(&qpt_arguments).build();
        Ok(GetSplitsResponse::new(
            request.catalog_name(),
            [split],
            None,
        ))
    }
}

pub async fn list_database_names(jdbc_connection: &dyn JdbcConnection) -> Result<Vec<String>> {
    let schemas = jdbc_connection.metadata().schemas(None, None).await?;
    let mut schema_names = IndexSet::new();
    for entry in schemas {
        let Some(schema_name) = entry.schema else {
            return Err(FederationError::connector(
                FederationSourceErrorCode::InvalidCredentialsException,
                "Invalid credentials was specified",
            ));
        };
        if schema_name != "information_schema" {
            schema_names.insert(schema_name);
        }
    }
    Ok(schema_names.into_iter().collect())
}

pub fn escape_name_pattern(name: Option<&str>, escape: Option<&str>) -> Result<Option<String>> {
    let (Some(name), Some(escape)) = (name, escape) else {
        return Ok(name.map(ToString::to_string));
    };
    if escape == "_" {
        return Err(FederationError::IllegalArgument(
            "Escape string must not be '_'".to_string(),
        ));
    }
    if escape == "%" {
        return Err(FederationError::IllegalArgument(
            "Escape string must not be '%'".to_string(),
        ));
    }
    let escaped_name = name
        .replace(escape, &format!("{escape}{escape}"))
        .replace('_', &format!("{escape}_"))
        .replace('%', &format!("{escape}%"));
    Ok(Some(escaped_name))
}

pub async fn columns(
    catalog_name: Option<&str>,
    table_handle: &TableName,
    metadata: &dyn DatabaseMetaData,
) -> Result<Vec<ColumnInfo>> {
    tracing::debug!(
        "getColumns, catalogName:{:?}, tableHandle: {}",
        catalog_name,
        table_handle
    );
    let escape = metadata.search_string_escape();
    let schema_pattern = escape_name_pattern(Some(table_handle.schema_name()), escape.as_deref())?;
    let table_pattern = escape_name_pattern(Some(table_handle.table_name()), escape.as_deref())?;
    Ok(metadata
        .columns(
            catalog_name,
            schema_pattern.as_deref(),
            table_pattern.as_deref(),
            None,
        )
        .await?)
}

#[async_trait]
impl<D: MetadataDialect> MetadataHandler for JdbcMetadataHandler<D> {
    fn context(&self) -> &MetadataContext {
        &self.context
    }

    async fn list_schema_names(&self, request: &ListSchemasRequest) -> Result<ListSchemasResponse> {
        let connection = self.connect(request.identity()).await?;
        tracing::info!(
            "{}: List schema names for Catalog {}",
            request.query_id().or_java_null(),
            request.catalog_name()
        );
        let result = self
            .list_database_names(connection.as_ref())
            .await
            .map(|names| ListSchemasResponse::new(request.catalog_name(), names));
        close_after(connection.as_ref(), result).await
    }

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse> {
        let connection = self.connect(request.identity()).await?;
        tracing::info!(
            "{}: List table names for Catalog {}, Schema {}",
            request.query_id().or_java_null(),
            request.catalog_name(),
            request.schema_name().or_java_null()
        );
        let result = async {
            if request.page_size() == UNLIMITED_PAGE_SIZE_VALUE && request.next_token().is_none() {
                let adjusted_schema_name = self
                    .adjusted_schema_name(connection.as_ref(), request.schema_name())
                    .await?;
                tracing::info!("doListTables - NO pagination");
                let tables = self
                    .list_schema_tables(connection.as_ref(), &adjusted_schema_name)
                    .await?;
                return Ok(ListTablesResponse::new(
                    request.catalog_name(),
                    tables,
                    None,
                ));
            }
            tracing::info!("doListTables - pagination");
            self.list_paginated_tables(connection.as_ref(), request)
                .await
        }
        .await;
        close_after(connection.as_ref(), result).await
    }

    async fn get_query_passthrough_schema(
        &self,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        if !request.is_query_passthrough() {
            return Err(FederationError::invalid_input(format!(
                "No Query passed through [{{}}]{request}"
            )));
        }
        JDBC_QUERY_PASSTHROUGH.verify(request.query_passthrough_arguments())?;
        let customer_passed_query = request
            .query_passthrough_arguments()
            .get(QUERY)
            .cloned()
            .flatten()
            .unwrap_or_default();
        let connection = self.connect(request.identity()).await?;
        let result = async {
            let prepared_statement = connection.prepare_statement(&customer_passed_query).await?;
            let metadata = connection.statement_metadata(&prepared_statement).await?;
            let Some(metadata) = metadata.as_deref() else {
                return Err(FederationError::not_supported(format!(
                    "Query not supported: ResultSetMetaData not available for query: {customer_passed_query}"
                )));
            };
            let config_options = self.config_options();
            let mut schema_builder = SchemaBuilder::new_builder();
            for column in metadata.columns() {
                let column_name = column.label().to_string();
                let precision = column.precision();
                let column_type = self
                    .dialect
                    .convert_datasource_type(column, config_options)?;
                match column_type {
                    Some(arrow_type) if minor_type_of(&arrow_type).is_supported() => {
                        if matches!(arrow_type, DataType::List(_)) {
                            schema_builder.add_list_field(
                                &column_name,
                                self.dialect.array_element_type(
                                    Some(column.table_name()),
                                    column.display_size(),
                                    precision,
                                ),
                            );
                        } else {
                            schema_builder.add_field(
                                FieldBuilder::new_builder(&column_name, arrow_type).build(),
                            );
                        }
                    }
                    other => {
                        tracing::warn!(
                            "getSchema: Unable to map type for column[{}] to a supported type, attempted {:?} - defaulting type to VARCHAR.",
                            column_name,
                            other
                        );
                        schema_builder.add_field(
                            FieldBuilder::new_builder(&column_name, DataType::Utf8).build(),
                        );
                    }
                }
            }
            Ok(GetTableResponse::new(
                request.catalog_name(),
                request.table_name().clone(),
                schema_builder.build(),
                IndexSet::new(),
            ))
        }
        .await;
        close_after(connection.as_ref(), result).await
    }

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse> {
        tracing::debug!("doGetTable getTableName:{}", request.table_name());
        let connection = self.connect(request.identity()).await?;
        let result = async {
            let partition_schema = self.partition_schema(request.catalog_name())?;
            let adjusted_table_name = self
                .case_resolver
                .get_adjusted_table_name_object(
                    connection.as_ref(),
                    &TableName::new(
                        request.table_name().schema_name(),
                        request.table_name().table_name(),
                    ),
                    self.config_options(),
                )
                .await?;
            let schema = self
                .schema(connection.as_ref(), &adjusted_table_name, &partition_schema)
                .await?;
            let partition_columns: IndexSet<String> = partition_schema
                .fields()
                .iter()
                .map(|f| f.name().clone())
                .collect();
            Ok(GetTableResponse::new(
                request.catalog_name(),
                adjusted_table_name,
                schema,
                partition_columns,
            ))
        }
        .await;
        close_after(connection.as_ref(), result).await
    }

    fn enhance_partition_schema(
        &self,
        partition_schema_builder: &mut SchemaBuilder,
        request: &GetTableLayoutRequest,
    ) {
        self.dialect
            .enhance_partition_schema(self, partition_schema_builder, request);
    }

    async fn get_partitions(
        &self,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.dialect
            .get_partitions(self, block_writer, request, query_status_checker)
            .await
    }

    async fn get_splits(&self, request: &GetSplitsRequest) -> Result<GetSplitsResponse> {
        self.dialect.get_splits(self, request).await
    }

    async fn get_data_source_capabilities(
        &self,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.dialect.capabilities(self, request)
    }
}

#[async_trait]
pub trait MetadataDelegate: Send + Sync {
    fn partition_schema(&self, catalog_name: &str) -> Result<Schema>;

    async fn close(&self) -> Result<()>;
}

#[async_trait]
impl<D: MetadataDialect> MetadataDelegate for JdbcMetadataHandler<D> {
    fn partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        JdbcMetadataHandler::partition_schema(self, catalog_name)
    }

    async fn close(&self) -> Result<()> {
        JdbcMetadataHandler::close(self).await
    }
}

pub async fn tables(
    connection: &dyn JdbcConnection,
    database_name: &str,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let sql = "SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ?";
    let mut prepared_statement = connection.prepare_statement(sql).await?;
    prepared_statement.bind(database_name);
    tracing::debug!(
        "Prepared Statement for getting tables in schema {} : {}",
        database_name,
        prepared_statement.sql()
    );
    table_metadata(connection, &prepared_statement, TABLES_AND_VIEWS).await
}

pub async fn table_metadata(
    connection: &dyn JdbcConnection,
    prepared_statement: &PreparedStatement,
    table_type: &str,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let listed = async {
        let mut result_set = connection.execute_query(prepared_statement).await?;
        let mut list = Vec::new();
        while let Some(row) = result_set.next_row().await? {
            list.push(read_schema_table_name(&row)?);
        }
        Ok(list)
    }
    .await;
    if let Err(ex) = &listed {
        tracing::warn!(
            "Unable to return list of {} from data source! {}",
            table_type,
            ex
        );
    }
    listed
}

pub fn schema_table_name(row: &Row) -> Result<TableName> {
    Ok(read_schema_table_name(row)?)
}

fn read_schema_table_name(row: &Row) -> std::result::Result<TableName, SqlError> {
    Ok(TableName::new(
        row.by_label("TABLE_SCHEM")?
            .to_java_string()
            .unwrap_or_default(),
        row.by_label("TABLE_NAME")?
            .to_java_string()
            .unwrap_or_default(),
    ))
}
