use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use indexmap::IndexSet;

use athena_federation_sdk::data::{
    ArrowType, BlockAllocator, FieldBuilder, SchemaBuilder, SupportedTypes,
};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{MetadataHandler, MetadataHandlerBase};
use athena_federation_sdk::metadata::optimizations::querypassthrough::QueryPassthroughSignature;
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableRequest, GetTableResponse, ListSchemasRequest,
    ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::security::AwsRequestOverrideConfiguration;
use athena_federation_sdk::util::{PaginationHelper, UNLIMITED_PAGE_SIZE_VALUE};
use athena_federation_sdk::{FederationError, Result};

use super::jdbc_arrow_type_converter::JdbcArrowTypeConverter;
use super::jdbc_util::JdbcUtil;
use crate::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use crate::qpt::{JdbcQueryPassthrough, QUERY};
use crate::resolver::{DefaultJdbcCaseResolver, JdbcCaseResolver, JdbcCaseResolverExt};
use crate::sql::{DatabaseMetaData, JdbcConnection, ResultSet, ResultSetMetaData};
use crate::validate::{close_after, null_pointer};

pub const TABLES_AND_VIEWS: &str = "Tables and Views";
const TABLE_TYPES: [&str; 4] = ["TABLE", "VIEW", "EXTERNAL TABLE", "MATERIALIZED VIEW"];

pub struct JdbcMetadataHandlerBase {
    metadata_base: MetadataHandlerBase,
    jdbc_connection_factory: Option<Arc<dyn JdbcConnectionFactory>>,
    database_connection_config: Option<DatabaseConnectionConfig>,
    case_resolver: Arc<dyn JdbcCaseResolver>,
    jdbc_query_passthrough: JdbcQueryPassthrough,
}

impl JdbcMetadataHandlerBase {
    pub fn new(
        metadata_base: MetadataHandlerBase,
        database_connection_config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
    ) -> JdbcMetadataHandlerBase {
        let case_resolver = Arc::new(DefaultJdbcCaseResolver::new(
            database_connection_config.engine(),
        ));
        JdbcMetadataHandlerBase::new_with_case_resolver(
            metadata_base,
            database_connection_config,
            jdbc_connection_factory,
            case_resolver,
        )
    }

    pub fn new_with_case_resolver(
        metadata_base: MetadataHandlerBase,
        database_connection_config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        case_resolver: Arc<dyn JdbcCaseResolver>,
    ) -> JdbcMetadataHandlerBase {
        JdbcMetadataHandlerBase {
            metadata_base,
            jdbc_connection_factory: Some(jdbc_connection_factory),
            database_connection_config: Some(database_connection_config),
            case_resolver,
            jdbc_query_passthrough: JdbcQueryPassthrough,
        }
    }

    pub fn new_without_connection(metadata_base: MetadataHandlerBase) -> JdbcMetadataHandlerBase {
        let case_resolver = Arc::new(DefaultJdbcCaseResolver::new(metadata_base.source_type()));
        JdbcMetadataHandlerBase::new_without_connection_with_case_resolver(
            metadata_base,
            case_resolver,
        )
    }

    pub fn new_without_connection_with_case_resolver(
        metadata_base: MetadataHandlerBase,
        case_resolver: Arc<dyn JdbcCaseResolver>,
    ) -> JdbcMetadataHandlerBase {
        JdbcMetadataHandlerBase {
            metadata_base,
            jdbc_connection_factory: None,
            database_connection_config: None,
            case_resolver,
            jdbc_query_passthrough: JdbcQueryPassthrough,
        }
    }

    pub fn metadata_base(&self) -> &MetadataHandlerBase {
        &self.metadata_base
    }

    pub fn jdbc_connection_factory(&self) -> Option<&Arc<dyn JdbcConnectionFactory>> {
        self.jdbc_connection_factory.as_ref()
    }

    pub fn database_connection_config(&self) -> Option<&DatabaseConnectionConfig> {
        self.database_connection_config.as_ref()
    }

    pub fn case_resolver(&self) -> &Arc<dyn JdbcCaseResolver> {
        &self.case_resolver
    }

    pub fn jdbc_query_passthrough(&self) -> &JdbcQueryPassthrough {
        &self.jdbc_query_passthrough
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        self.metadata_base.config_options()
    }
}

#[async_trait]
pub trait JdbcMetadataHandler: MetadataHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase;

    async fn list_database_names(
        &self,
        jdbc_connection: &dyn JdbcConnection,
    ) -> Result<Vec<String>> {
        let mut result_set = jdbc_connection.metadata().get_schemas().await?;
        let mut schema_names = IndexSet::new();
        while result_set.next().await? {
            let Some(schema_name) = result_set.get_string("TABLE_SCHEM")? else {
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

    async fn list_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        list_tables_request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        let base = self.jdbc_metadata_base();
        let adjusted_schema_name = base
            .case_resolver()
            .get_adjusted_schema_name_string(
                connection,
                list_tables_request.schema_name().unwrap_or_default(),
                base.config_options(),
            )
            .await?;
        tracing::debug!(
            "Request is asking for pagination, but true pagination has not been implemented."
        );
        let page_size = list_tables_request.page_size();
        let start_token = list_tables_request.next_token();
        let all_tables = self.list_tables(connection, &adjusted_schema_name).await?;
        let (catalog_name, tables, next_token) = PaginationHelper::manual_pagination(
            &all_tables,
            start_token,
            page_size,
            list_tables_request.catalog_name(),
        )?
        .into_parts();
        Ok(ListTablesResponse::new(catalog_name, tables, next_token))
    }

    async fn list_tables(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        tracing::debug!("listTables, schemaName:{}", database_name);
        let metadata = jdbc_connection.metadata();
        let escape = metadata.get_search_string_escape()?;
        let catalog = jdbc_connection.get_catalog().await?;
        let schema_pattern = self.escape_name_pattern(Some(database_name), escape.as_deref())?;
        let mut result_set = metadata
            .get_tables(
                catalog.as_deref(),
                schema_pattern.as_deref(),
                None,
                Some(&TABLE_TYPES),
            )
            .await?;
        let mut list = Vec::new();
        while result_set.next().await? {
            list.push(JdbcUtil::get_schema_table_name(&result_set)?);
        }
        Ok(list)
    }

    fn escape_name_pattern(
        &self,
        name: Option<&str>,
        escape: Option<&str>,
    ) -> Result<Option<String>> {
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

    fn convert_datasource_type_to_arrow(
        &self,
        column_index: usize,
        precision: i32,
        config_options: &HashMap<String, String>,
        metadata: &ResultSetMetaData,
    ) -> Result<Option<ArrowType>> {
        let scale = metadata.scale(column_index)?;
        let column_type = metadata.column_type(column_index)?;
        JdbcArrowTypeConverter::to_arrow_type(column_type, precision, scale, config_options)
    }

    async fn get_schema(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
        _request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Schema> {
        let config_options = self.jdbc_metadata_base().config_options();
        let mut schema_builder = SchemaBuilder::new_builder();
        let catalog = jdbc_connection.get_catalog().await?;
        let mut result_set = self
            .get_columns(catalog.as_deref(), table_name, jdbc_connection.metadata())
            .await?;
        let mut found = false;
        while result_set.next().await? {
            let column_type = JdbcArrowTypeConverter::to_arrow_type(
                result_set.get_int("DATA_TYPE")?.unwrap_or(0),
                result_set.get_int("COLUMN_SIZE")?.unwrap_or(0),
                result_set.get_int("DECIMAL_DIGITS")?.unwrap_or(0),
                config_options,
            )?;
            let column_name = result_set
                .get_string("COLUMN_NAME")?
                .unwrap_or_else(|| "null".to_string());
            match column_type {
                Some(arrow_type) if SupportedTypes::is_supported_arrow_type(&arrow_type) => {
                    if matches!(arrow_type, DataType::List(_)) {
                        let type_name = result_set.get_string("TYPE_NAME")?;
                        schema_builder.add_list_field(
                            &column_name,
                            self.get_array_arrow_type_from_type_name(
                                type_name.as_deref(),
                                result_set.get_int("COLUMN_SIZE")?.unwrap_or(0),
                                result_set.get_int("DECIMAL_DIGITS")?.unwrap_or(0),
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
            found = true;
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

    async fn get_columns(
        &self,
        catalog_name: Option<&str>,
        table_handle: &TableName,
        metadata: &dyn DatabaseMetaData,
    ) -> Result<ResultSet> {
        tracing::debug!(
            "getColumns, catalogName:{:?}, tableHandle: {}",
            catalog_name,
            table_handle
        );
        let escape = metadata.get_search_string_escape()?;
        let schema_pattern =
            self.escape_name_pattern(Some(table_handle.schema_name()), escape.as_deref())?;
        let table_pattern =
            self.escape_name_pattern(Some(table_handle.table_name()), escape.as_deref())?;
        Ok(metadata
            .get_columns(
                catalog_name,
                schema_pattern.as_deref(),
                table_pattern.as_deref(),
                None,
            )
            .await?)
    }

    fn get_partition_schema(&self, catalog_name: &str) -> Result<Schema>;

    fn get_array_arrow_type_from_type_name(
        &self,
        _type_name: Option<&str>,
        _precision: i32,
        _scale: i32,
    ) -> ArrowType {
        DataType::Utf8
    }

    async fn setup_query_passthrough_split(
        &self,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        let spill_location = self.make_spill_location(request)?;
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        let encryption_key = self
            .make_encryption_key_with_aws_request_override_configuration(override_config.as_ref())
            .await?;
        let qpt_arguments: HashMap<String, String> = request
            .constraints()
            .query_passthrough_arguments()
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
            .collect();
        let split = Split::new_builder(Some(spill_location), encryption_key)
            .apply_properties(&qpt_arguments)
            .build();
        Ok(GetSplitsResponse::new(
            request.catalog_name(),
            [split],
            None,
        ))
    }

    fn wrap_name_with_escaped_character(&self, input: &str) -> String {
        input.to_string()
    }

    async fn close(&self) -> Result<()> {
        match self.jdbc_metadata_base().jdbc_connection_factory() {
            Some(factory) => factory.close().await,
            None => Ok(()),
        }
    }
}

#[async_trait]
pub trait JdbcMetadataHandlerExt: JdbcMetadataHandler {
    fn jdbc_database_connection_secret(&self) -> Option<&str>;

    async fn jdbc_get_connection(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>>;

    async fn jdbc_do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse>;

    async fn jdbc_do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse>;

    async fn jdbc_do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse>;

    async fn jdbc_do_get_query_passthrough_schema(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse>;
}

fn missing_factory() -> FederationError {
    null_pointer(
        "Cannot invoke \"com.amazonaws.athena.connectors.jdbc.connection.JdbcConnectionFactory.getConnection(com.amazonaws.athena.connector.credentials.CredentialsProvider)\" because \"this.jdbcConnectionFactory\" is null",
    )
}

#[async_trait]
impl<T: JdbcMetadataHandler + ?Sized> JdbcMetadataHandlerExt for T {
    fn jdbc_database_connection_secret(&self) -> Option<&str> {
        self.jdbc_metadata_base()
            .database_connection_config()
            .and_then(DatabaseConnectionConfig::secret)
    }

    async fn jdbc_get_connection(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Box<dyn JdbcConnection>> {
        let factory = self
            .jdbc_metadata_base()
            .jdbc_connection_factory()
            .ok_or_else(missing_factory)?;
        let override_config = self
            .get_request_override_config_with_config_options(
                identity.map(FederatedIdentity::config_options),
            )
            .await?;
        let credentials = self
            .get_credential_provider_with_request_override_configuration(override_config.as_ref())
            .await?;
        factory.get_connection(credentials.as_deref()).await
    }

    async fn jdbc_do_list_schema_names(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        let connection = self.jdbc_get_connection(request.identity()).await?;
        tracing::info!(
            "{}: List schema names for Catalog {}",
            request.query_id().unwrap_or("null"),
            request.catalog_name()
        );
        let result = self
            .list_database_names(connection.as_ref())
            .await
            .map(|names| ListSchemasResponse::new(request.catalog_name(), names));
        close_after(connection.as_ref(), result).await
    }

    async fn jdbc_do_list_tables(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        let connection = self.jdbc_get_connection(request.identity()).await?;
        tracing::info!(
            "{}: List table names for Catalog {}, Schema {}",
            request.query_id().unwrap_or("null"),
            request.catalog_name(),
            request.schema_name().unwrap_or("null")
        );
        let result = async {
            if request.page_size() == UNLIMITED_PAGE_SIZE_VALUE && request.next_token().is_none() {
                let base = self.jdbc_metadata_base();
                let adjusted_schema_name = base
                    .case_resolver()
                    .get_adjusted_schema_name_string(
                        connection.as_ref(),
                        request.schema_name().unwrap_or_default(),
                        base.config_options(),
                    )
                    .await?;
                tracing::info!("doListTables - NO pagination");
                let tables = self
                    .list_tables(connection.as_ref(), &adjusted_schema_name)
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

    async fn jdbc_do_get_table(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        tracing::debug!("doGetTable getTableName:{}", request.table_name());
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        let factory = self
            .jdbc_metadata_base()
            .jdbc_connection_factory()
            .ok_or_else(missing_factory)?;
        let credentials = self
            .get_credential_provider_with_request_override_configuration(override_config.as_ref())
            .await?;
        let connection = factory.get_connection(credentials.as_deref()).await?;
        let result = async {
            let partition_schema = self.get_partition_schema(request.catalog_name())?;
            let base = self.jdbc_metadata_base();
            let adjusted_table_name = base
                .case_resolver()
                .get_adjusted_table_name_object(
                    connection.as_ref(),
                    &TableName::new(
                        request.table_name().schema_name(),
                        request.table_name().table_name(),
                    ),
                    base.config_options(),
                )
                .await?;
            let schema = self
                .get_schema(
                    connection.as_ref(),
                    &adjusted_table_name,
                    &partition_schema,
                    override_config.as_ref(),
                )
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

    async fn jdbc_do_get_query_passthrough_schema(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        if !request.is_query_passthrough() {
            return Err(FederationError::invalid_input(format!(
                "No Query passed through [{{}}]{request}"
            )));
        }
        let base = self.jdbc_metadata_base();
        base.jdbc_query_passthrough()
            .verify(request.query_passthrough_arguments())?;
        let customer_passed_query = request
            .query_passthrough_arguments()
            .get(QUERY)
            .cloned()
            .flatten()
            .unwrap_or_default();
        let connection = self.jdbc_get_connection(request.identity()).await?;
        let result = async {
            let prepared_statement = connection.prepare_statement(&customer_passed_query).await?;
            let Some(metadata) = prepared_statement.metadata() else {
                return Err(FederationError::not_supported(format!(
                    "Query not supported: ResultSetMetaData not available for query: {customer_passed_query}"
                )));
            };
            let config_options = base.config_options();
            let mut schema_builder = SchemaBuilder::new_builder();
            for column_index in 1..=metadata.column_count() {
                let column_name = metadata.column_name(column_index)?;
                let column_label = metadata.column_label(column_index)?;
                let column_name = if column_name == column_label {
                    column_name.to_string()
                } else {
                    column_label.to_string()
                };
                let precision = metadata.precision(column_index)?;
                let column_type =
                    self.convert_datasource_type_to_arrow(column_index, precision, config_options, metadata)?;
                match column_type {
                    Some(arrow_type) if SupportedTypes::is_supported_arrow_type(&arrow_type) => {
                        if matches!(arrow_type, DataType::List(_)) {
                            let table_name = metadata.table_name(column_index)?.to_string();
                            schema_builder.add_list_field(
                                &column_name,
                                self.get_array_arrow_type_from_type_name(
                                    Some(&table_name),
                                    metadata.column_display_size(column_index)?,
                                    precision,
                                ),
                            );
                        } else {
                            schema_builder.add_field(FieldBuilder::new_builder(&column_name, arrow_type).build());
                        }
                    }
                    other => {
                        tracing::warn!(
                            "getSchema: Unable to map type for column[{}] to a supported type, attempted {:?} - defaulting type to VARCHAR.",
                            column_name,
                            other
                        );
                        schema_builder.add_field(FieldBuilder::new_builder(&column_name, DataType::Utf8).build());
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
}
