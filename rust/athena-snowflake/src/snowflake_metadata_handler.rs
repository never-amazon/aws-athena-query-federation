use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::connection::EnvironmentConstants;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::{
    ArrowType, Block, BlockAllocator, BlockWriter, FieldBuilder, SchemaBuilder, SupportedTypes,
};
use athena_federation_sdk::domain::predicate::functions::StandardFunctions;
use athena_federation_sdk::domain::split::{Split, SplitBuilder};
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{
    AwsClients, FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
};
use athena_federation_sdk::metadata::optimizations::pushdown::{
    ComplexExpressionPushdownSubType, FilterPushdownSubType, LimitPushdownSubType,
    PushdownSubTypes, TopNPushdownSubType,
};
use athena_federation_sdk::metadata::optimizations::{
    DataSourceOptimizations, OptimizationSubType,
};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, CachableSecretsManager, KmsEncryptionProvider,
};
use athena_federation_sdk::serde::{schema_from_bytes, schema_to_bytes, IpcFormat};
use athena_federation_sdk::util::{PaginationHelper, UNLIMITED_PAGE_SIZE_VALUE};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, JdbcConnectionFactory,
};
use athena_jdbc::manager::{
    JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerExt, JdbcSplitQueryBuilder,
    JdbcUtil, PreparedStatementBuilder, TABLES_AND_VIEWS,
};
use athena_jdbc::qpt::{JDBC_QUERY_PASSTHROUGH, QUERY};
use athena_jdbc::resolver::{DefaultJdbcCaseResolver, JdbcCaseResolver, JdbcCaseResolverExt};
use athena_jdbc::sql::{JdbcConnection, ResultSetMetaData, SqlException};
use indexmap::IndexMap;

use crate::connection::SnowflakeConnectionFactory;
use crate::resolver::SnowflakeJdbcCaseResolver;
use crate::s3_export::{S3ObjectLister, S3ObjectSummary, S3Uri};
use crate::snowflake_constants::{is_blank, SnowflakeConstants};
use crate::snowflake_credentials_provider::SnowflakeCredentialsProvider;
use crate::snowflake_environment_properties::SnowflakeEnvironmentProperties;
use crate::snowflake_federation_expression_parser::SnowflakeFederationExpressionParser;
use crate::snowflake_query_string_builder::SnowflakeQueryStringBuilder;
use crate::sql::SNOWFLAKE_SQL_EXCEPTION;
use crate::utils::SnowflakeArrowTypeConverter;

const MAX_SPLITS_PER_REQUEST: usize = 1_000_000;
const COLUMN_NAME: &str = "COLUMN_NAME";
const COUNTS_COLUMN_NAME: &str = "COUNTS";
const PRIMARY_KEY_COLUMN_NAME: &str = "column_name";
const ALL_PARTITIONS: &str = "*";
const FILES_ALREADY_EXISTING: &str = "Files already existing";
const INTEGRATION_NOT_FOUND: &str = "does not exist or not authorized";
const MISSING_S3_CLIENT: &str =
    "Cannot invoke \"software.amazon.awssdk.services.s3.S3Client.utilities()\" because \"this.amazonS3\" is null";

pub struct SnowflakeMetadataHandler {
    base: JdbcMetadataHandlerBase,
    amazon_s3: Option<Arc<dyn S3ObjectLister>>,
    snowflake_query_string_builder: SnowflakeQueryStringBuilder,
}

fn java_text(error: &FederationError) -> String {
    error.java_message().unwrap_or_else(|| "null".to_string())
}

fn snowflake_sql_exception(error: &FederationError) -> Option<&SqlException> {
    SqlException::from_federation_error(error)
        .filter(|sql| sql.class_name() == SNOWFLAKE_SQL_EXCEPTION)
}

fn identity_option<'a>(
    identity: Option<&'a FederatedIdentity>,
    key: &str,
) -> Option<&'a Option<String>> {
    identity?.config_options().get(key)
}

fn missing_field_reader() -> FederationError {
    FederationError::null_pointer(
        "Cannot invoke \"org.apache.arrow.vector.complex.reader.FieldReader.setPosition(int)\" because \"locationReader\" is null",
    )
}

async fn close_after<T>(connection: &dyn JdbcConnection, result: Result<T>) -> Result<T> {
    let closed = connection.close().await;
    match (result, closed) {
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e.into()),
        (Ok(value), Ok(())) => Ok(value),
    }
}

impl SnowflakeMetadataHandler {
    pub fn connection_factory(
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<SnowflakeConnectionFactory> {
        SnowflakeConnectionFactory::new_with_config_options(
            config.clone(),
            Some(SnowflakeEnvironmentProperties::get_snow_flake_parameter(
                &SnowflakeConstants::jdbc_properties(),
                config_options,
            )),
            DatabaseConnectionInfo::new(
                SnowflakeConstants::SNOWFLAKE_DRIVER_CLASS,
                SnowflakeConstants::SNOWFLAKE_DEFAULT_PORT,
            )?,
            Some(config_options),
        )
    }

    pub async fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeMetadataHandler> {
        let config = JdbcUtil::get_single_database_config_from_env(
            SnowflakeConstants::SNOWFLAKE_NAME,
            &config_options,
        )?;
        let mut handler = SnowflakeMetadataHandler::new(clients, config, config_options)?;
        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        handler.amazon_s3 = Some(Arc::new(aws_sdk_s3::Client::new(&sdk_config)));
        Ok(handler)
    }

    pub fn new(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeMetadataHandler> {
        let factory = Arc::new(SnowflakeMetadataHandler::connection_factory(
            &config,
            &config_options,
        )?);
        SnowflakeMetadataHandler::with_connection_factory(clients, config, factory, config_options)
    }

    pub fn with_connection_factory(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeMetadataHandler> {
        SnowflakeMetadataHandler::with_case_resolver(
            clients,
            config,
            jdbc_connection_factory,
            config_options,
            Arc::new(SnowflakeJdbcCaseResolver::new(
                SnowflakeConstants::SNOWFLAKE_NAME,
            )),
            None,
        )
    }

    pub fn with_s3_client(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        s3_client: Arc<dyn S3ObjectLister>,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeMetadataHandler> {
        let case_resolver = Arc::new(DefaultJdbcCaseResolver::new(config.engine()));
        SnowflakeMetadataHandler::with_case_resolver(
            clients,
            config,
            jdbc_connection_factory,
            config_options,
            case_resolver,
            Some(s3_client),
        )
    }

    pub fn with_case_resolver(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config_options: HashMap<String, String>,
        case_resolver: Arc<dyn JdbcCaseResolver>,
        amazon_s3: Option<Arc<dyn S3ObjectLister>>,
    ) -> Result<SnowflakeMetadataHandler> {
        let metadata_base = MetadataHandlerBase::builder(config.engine(), clients)
            .config_options(config_options)
            .build()?;
        Ok(SnowflakeMetadataHandler {
            base: JdbcMetadataHandlerBase::new_with_case_resolver(
                metadata_base,
                config,
                jdbc_connection_factory,
                case_resolver,
            ),
            amazon_s3,
            snowflake_query_string_builder: SnowflakeQueryStringBuilder::new(
                SnowflakeConstants::DOUBLE_QUOTE_CHAR,
                Arc::new(SnowflakeFederationExpressionParser::new(
                    SnowflakeConstants::DOUBLE_QUOTE_CHAR,
                )),
            ),
        })
    }

    fn config_options(&self) -> &HashMap<String, String> {
        self.base.config_options()
    }

    fn s3_export_enabled(&self) -> bool {
        SnowflakeConstants::is_s3_export_enabled(self.config_options())
    }

    pub fn get_sf_storage_integration_name_from_config(&self) -> Option<&str> {
        self.config_options()
            .get(SnowflakeConstants::STORAGE_INTEGRATION_CONFIG_KEY)
            .map(String::as_str)
    }

    fn storage_integration_name(&self) -> Result<String> {
        self.get_sf_storage_integration_name_from_config()
            .map(ToString::to_string)
            .ok_or_else(|| {
                FederationError::connector(
                    FederationSourceErrorCode::InvalidInputException,
                    "Snowflake storage integration name not found",
                )
            })
    }

    pub fn create_snowflake_credentials_provider(
        &self,
        secret_name: Option<&str>,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Option<SnowflakeCredentialsProvider> {
        let secret_name = secret_name.filter(|name| !is_blank(Some(name)))?;
        Some(SnowflakeCredentialsProvider::new(
            secret_name,
            Arc::clone(self.cachable_secrets_manager().secrets_manager()),
            request_override_configuration.cloned(),
        ))
    }

    pub async fn get_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        database_name: &str,
        offset: i32,
        limit: i32,
    ) -> std::result::Result<Vec<TableName>, SqlException> {
        let mut statement = connection
            .prepare_statement(SnowflakeConstants::LIST_PAGINATED_TABLES_QUERY)
            .await?;
        statement.set_string(1, database_name)?;
        statement.set_int(2, limit)?;
        statement.set_int(3, offset)?;
        JdbcUtil::get_table_metadata(connection, &statement, TABLES_AND_VIEWS).await
    }

    pub async fn get_list_exported_objects(
        &self,
        s3_export_bucket_name: &str,
        prefix: &str,
    ) -> Result<Vec<S3ObjectSummary>> {
        let lister = self
            .amazon_s3
            .as_ref()
            .ok_or_else(|| FederationError::null_pointer(MISSING_S3_CLIENT))?;
        lister
            .list_objects(s3_export_bucket_name, prefix)
            .await
            .map_err(|e| {
                let error_message = format!(
                    "Failed to list objects in bucket {s3_export_bucket_name} with prefix {prefix}"
                );
                tracing::error!("{}: {}", error_message, e);
                FederationError::Runtime(error_message)
            })
    }

    pub async fn get_storage_integration_properties(
        &self,
        connection: &dyn JdbcConnection,
        integration_name: &str,
    ) -> Result<Option<HashMap<String, String>>> {
        let check_integration_query = SnowflakeConstants::DESCRIBE_STORAGE_INTEGRATION_TEMPLATE
            .replace(
                "%s",
                &self
                    .snowflake_query_string_builder
                    .quote(&integration_name.to_uppercase()),
            );
        let described: std::result::Result<HashMap<String, String>, SqlException> = async {
            let mut result_set = connection
                .execute_query_with_sql(&check_integration_query)
                .await?;
            let mut storage_integration_row = HashMap::new();
            while result_set.next().await? {
                storage_integration_row.insert(
                    result_set
                        .get_string(SnowflakeConstants::STORAGE_INTEGRATION_PROPERTY_KEY)?
                        .unwrap_or_else(|| "null".to_string()),
                    result_set
                        .get_string(SnowflakeConstants::STORAGE_INTEGRATION_PROPERTY_VALUE_KEY)?
                        .unwrap_or_else(|| "null".to_string()),
                );
            }
            Ok(storage_integration_row)
        }
        .await;
        match described {
            Ok(row) => Ok(Some(row)),
            Err(e) => {
                tracing::error!(
                    "Error checking for integration {}: exception:{}, message: {}",
                    integration_name,
                    e.class_name(),
                    e.message()
                );
                if e.message().contains(INTEGRATION_NOT_FOUND) {
                    return Ok(None);
                }
                Err(e.into())
            }
        }
    }

    fn validate_sf_storage_integration_exist_and_valid(
        storage_integration_map: &HashMap<String, String>,
    ) -> Result<()> {
        let bucket_key = SnowflakeConstants::STORAGE_INTEGRATION_BUCKET_KEY;
        let provider_key = SnowflakeConstants::STORAGE_INTEGRATION_STORAGE_PROVIDER_KEY;
        let s3_export_path = storage_integration_map.get(bucket_key).ok_or_else(|| {
            FederationError::illegal_argument(format!(
                "Snowflake Storage Integration, field:{bucket_key} cannot be null"
            ))
        })?;
        let provider = storage_integration_map.get(provider_key).ok_or_else(|| {
            FederationError::illegal_argument(format!(
                "Snowflake Storage Integration, field:{provider_key} cannot be null"
            ))
        })?;
        if !"S3".eq_ignore_ascii_case(provider) {
            return Err(FederationError::illegal_argument(format!(
                "Snowflake Storage Integration, field:{provider_key} must be S3"
            )));
        }
        if !s3_export_path.starts_with("s3://") {
            return Err(FederationError::illegal_argument(format!(
                "Storage integration bucket path must be an S3 path: {s3_export_path}"
            )));
        }
        if java_split_count(s3_export_path, ", ") != 1 {
            return Err(FederationError::illegal_argument(format!(
                "Snowflake Storage Integration, field:{bucket_key} must be a single S3 path"
            )));
        }
        Ok(())
    }

    pub async fn get_storage_integration_s3_path_from_snow_flake(
        &self,
        connection: &dyn JdbcConnection,
        integration_name: &str,
    ) -> Result<String> {
        let Some(properties) = self
            .get_storage_integration_properties(connection, integration_name)
            .await?
        else {
            return Err(FederationError::illegal_argument(format!(
                "Snowflake Storage Integration: name:{integration_name} not found"
            )));
        };
        SnowflakeMetadataHandler::validate_sf_storage_integration_exist_and_valid(&properties)?;
        let mut bucket_path = properties
            .get(SnowflakeConstants::STORAGE_INTEGRATION_BUCKET_KEY)
            .cloned()
            .unwrap_or_default();
        if bucket_path.ends_with('/') {
            bucket_path.pop();
        }
        Ok(bucket_path)
    }

    fn get_snow_flake_copy_into_base_sql(&self, request: &GetSplitsRequest) -> Result<String> {
        let constraints = request.constraints();
        if constraints.is_query_pass_through() {
            JDBC_QUERY_PASSTHROUGH.verify(constraints.query_passthrough_arguments())?;
            return Ok(constraints
                .query_passthrough_arguments()
                .get(QUERY)
                .cloned()
                .flatten()
                .unwrap_or_else(|| "null".to_string()));
        }
        let reader = request
            .partitions()
            .get_field_reader(SnowflakeConstants::S3_ENHANCED_PARTITION_COLUMN_NAME)
            .ok_or_else(|| {
                FederationError::null_pointer(
                    "Cannot invoke \"org.apache.arrow.vector.complex.reader.FieldReader.readByteArray()\" because \"fieldReaderPreparedStmt\" is null",
                )
            })?;
        let bytes = reader.read_byte_array().ok_or_else(|| {
            FederationError::null_pointer(
                "Cannot invoke \"java.nio.ByteBuffer.wrap(byte[])\" because the return value of \"org.apache.arrow.vector.complex.reader.FieldReader.readByteArray()\" is null",
            )
        })?;
        let schema = schema_from_bytes(&bytes)?;
        let generated = self
            .snowflake_query_string_builder
            .get_base_export_sql_string(
                request.catalog_name(),
                request.table_name().schema_name(),
                request.table_name().table_name(),
                &schema,
                constraints,
            )?;
        Ok(generated.unwrap_or_else(|| "null".to_string()))
    }

    async fn make_split_builder(&self, request: &GetSplitsRequest) -> Result<SplitBuilder> {
        let spill_location = self.make_spill_location(request)?;
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        let encryption_key = self
            .make_encryption_key_with_aws_request_override_configuration(override_config.as_ref())
            .await?;
        Ok(Split::new_builder(Some(spill_location), encryption_key))
    }

    async fn handle_direct_query_splits(
        &self,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        tracing::info!(
            "doGetSplits: {}: Catalog {}, table {}",
            request.query_id().unwrap_or("null"),
            request.table_name().schema_name(),
            request.table_name().table_name()
        );
        if request.constraints().is_query_pass_through() {
            tracing::info!("QPT Split Requested");
            return self.setup_query_passthrough_split(request).await;
        }
        let partition_contd = match request.continuation_token() {
            Some(token) if request.has_continuation_token() => {
                let parsed = token.parse::<i32>().map_err(|_| {
                    FederationError::NumberFormat(format!("For input string: \"{token}\""))
                })?;
                usize::try_from(parsed).unwrap_or(0)
            }
            _ => 0,
        };
        let partitions = request.partitions();
        let mut splits = Vec::new();
        for cur_partition in partition_contd..partitions.row_count() {
            let partition = {
                let location_reader = partitions
                    .get_field_reader(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME)
                    .ok_or_else(missing_field_reader)?;
                location_reader.set_position(cur_partition);
                location_reader.read_text()
            };
            let mut builder = self.make_split_builder(request).await?;
            tracing::info!(
                "{}: Input partition is {:?}",
                request.query_id().unwrap_or("null"),
                partition
            );
            builder = builder.add(
                SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
                partition.unwrap_or_else(|| "null".to_string()),
            );
            if let Some(filter) = identity_option(
                request.identity(),
                EnvironmentConstants::CATALOG_CASING_FILTER,
            ) {
                tracing::info!("Catalog Casing Filter found: {:?}", filter);
                builder = builder.add(
                    EnvironmentConstants::CATALOG_CASING_FILTER,
                    filter.clone().unwrap_or_else(|| "null".to_string()),
                );
            }
            splits.push(builder.build());
            if splits.len() >= MAX_SPLITS_PER_REQUEST {
                return Ok(GetSplitsResponse::new(
                    request.catalog_name(),
                    splits,
                    Some((cur_partition + 1).to_string()),
                ));
            }
        }
        Ok(GetSplitsResponse::new(request.catalog_name(), splits, None))
    }

    async fn run_export(
        &self,
        request: &GetSplitsRequest,
        s3_uri: &mut Option<S3Uri>,
    ) -> Result<()> {
        let query_id = request.query_id().unwrap_or("null");
        let connection = self.jdbc_get_connection(request.identity()).await?;
        let result = async {
            let sf_integration_name = self.storage_integration_name()?;
            let sf_s3_export_path_prefix = self
                .get_storage_integration_s3_path_from_snow_flake(
                    connection.as_ref(),
                    &sf_integration_name,
                )
                .await?;
            let snowflake_export_sql = self.get_snow_flake_copy_into_base_sql(request)?;
            let s3_path = format!(
                "{sf_s3_export_path_prefix}/{query_id}/{}/",
                uuid::Uuid::new_v4()
            );
            let snowflake_export_query = fill_template(
                SnowflakeConstants::COPY_INTO_QUERY_TEMPLATE,
                &[
                    &self.snowflake_query_string_builder.single_quote(&s3_path),
                    &snowflake_export_sql,
                    &self
                        .snowflake_query_string_builder
                        .quote(&sf_integration_name),
                ],
            );
            tracing::info!(
                "Snowflake Copy Statement: {} for queryId: {}",
                snowflake_export_query,
                query_id
            );
            if self.amazon_s3.is_none() {
                return Err(FederationError::null_pointer(MISSING_S3_CLIENT));
            }
            *s3_uri = Some(S3Uri::parse(&s3_path)?);
            let statement = connection
                .prepare_statement(&snowflake_export_query)
                .await?;
            connection.execute(&statement).await?;
            Ok(())
        }
        .await;
        close_after(connection.as_ref(), result).await
    }

    async fn handle_s3_export_splits(
        &self,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        let query_id = request.query_id().unwrap_or("null");
        let mut s3_uri = None;
        if let Err(error) = self.run_export(request, &mut s3_uri).await {
            match snowflake_sql_exception(&error) {
                Some(sql) if sql.message().contains(FILES_ALREADY_EXISTING) => {}
                Some(sql) => {
                    return Err(FederationError::connector(
                        FederationSourceErrorCode::InternalServiceException,
                        format!("Exception in execution export statement {}", sql.message()),
                    ));
                }
                None => {
                    return Err(FederationError::connector(
                        FederationSourceErrorCode::InternalServiceException,
                        format!(
                            "Exception in execution export statement :{}",
                            java_text(&error)
                        ),
                    ));
                }
            }
        }
        let Some(s3_uri) = s3_uri else {
            return Err(FederationError::connector(
                FederationSourceErrorCode::InvalidInputException,
                "S3 URI should not be empty for Snowflake S3 Export",
            ));
        };
        let no_value = || FederationError::Runtime("No value present".to_string());
        let bucket = s3_uri.bucket().ok_or_else(no_value)?;
        let key = s3_uri.key().ok_or_else(no_value)?;
        let s3_object_summaries = self.get_list_exported_objects(bucket, key).await?;
        tracing::debug!(
            "{} s3ObjectSummaries returned after executing on SnowFlake for queryId {}",
            s3_object_summaries.len(),
            query_id
        );
        let mut splits = Vec::new();
        if s3_object_summaries.is_empty() {
            tracing::debug!(
                "s3ObjectSummaries returned empty on SnowFlake for queryId {}",
                query_id
            );
            splits.push(self.export_split(request, bucket, "").await?);
        } else {
            for object_summary in &s3_object_summaries {
                splits.push(
                    self.export_split(request, bucket, object_summary.key())
                        .await?,
                );
            }
        }
        Ok(GetSplitsResponse::new(request.catalog_name(), splits, None))
    }

    async fn export_split(
        &self,
        request: &GetSplitsRequest,
        bucket: &str,
        key: &str,
    ) -> Result<Split> {
        Ok(self
            .make_split_builder(request)
            .await?
            .add(
                SnowflakeConstants::SNOWFLAKE_SPLIT_QUERY_ID,
                request.query_id().unwrap_or("null"),
            )
            .add(SnowflakeConstants::SNOWFLAKE_SPLIT_EXPORT_BUCKET, bucket)
            .add(SnowflakeConstants::SNOWFLAKE_SPLIT_OBJECT_KEY, key)
            .build())
    }

    async fn check_for_view(
        &self,
        table_name: &TableName,
        connection: &dyn JdbcConnection,
    ) -> Result<bool> {
        let statement = PreparedStatementBuilder::new()
            .with_connection(connection)
            .with_query(SnowflakeConstants::VIEW_CHECK_QUERY)?
            .with_parameters(vec![
                table_name.schema_name().to_string(),
                table_name.table_name().to_string(),
            ])?
            .build()
            .await?;
        let mut result_set = connection.execute_query(&statement).await?;
        let view_flag = result_set.next().await?;
        tracing::info!("viewFlag: {}", view_flag);
        Ok(view_flag)
    }

    async fn get_primary_key(
        &self,
        table_name: &TableName,
        connection: &dyn JdbcConnection,
    ) -> Result<Option<String>> {
        tracing::debug!("getPrimaryKey tableName: {}", table_name);
        let sql = format!(
            "{}{}",
            SnowflakeConstants::SHOW_PRIMARY_KEYS_QUERY,
            self.quoted_table_name(table_name)
        );
        let statement = connection.prepare_statement(&sql).await?;
        let mut result_set = connection.execute_query(&statement).await?;
        let mut primary_keys = Vec::new();
        while result_set.next().await? {
            let key = result_set
                .get_string(PRIMARY_KEY_COLUMN_NAME)?
                .unwrap_or_else(|| "null".to_string());
            primary_keys.push(self.snowflake_query_string_builder.quote(&key));
        }
        let primary_key_string = primary_keys.join(",");
        if !primary_key_string.is_empty()
            && self
                .has_unique_primary_key(table_name, &primary_key_string, connection)
                .await
        {
            return Ok(Some(primary_key_string));
        }
        Ok(None)
    }

    fn quoted_table_name(&self, table_name: &TableName) -> String {
        format!(
            "{}.{}",
            self.snowflake_query_string_builder
                .quote(table_name.schema_name()),
            self.snowflake_query_string_builder
                .quote(table_name.table_name())
        )
    }

    async fn has_unique_primary_key(
        &self,
        table_name: &TableName,
        primary_key: &str,
        connection: &dyn JdbcConnection,
    ) -> bool {
        let sql = format!(
            "SELECT {primary_key}, count(*) as COUNTS FROM {} GROUP BY {primary_key} ORDER BY COUNTS DESC LIMIT 1",
            self.quoted_table_name(table_name)
        );
        let checked: std::result::Result<bool, SqlException> = async {
            let mut statement = connection.prepare_statement(&sql).await?;
            statement.set_query_timeout(SnowflakeConstants::UNIQUENESS_CHECK_TIMEOUT_SECONDS)?;
            let mut result_set = connection.execute_query(&statement).await?;
            Ok(result_set.next().await?
                && result_set.get_int(COUNTS_COLUMN_NAME)?.unwrap_or(0) == 1)
        }
        .await;
        match checked {
            Ok(true) => true,
            Ok(false) => {
                tracing::warn!(
                    "Primary key ,{}, is not unique. Falling back to single partition...",
                    primary_key
                );
                false
            }
            Err(e) => {
                tracing::warn!(
                    "Primary key uniqueness check timed out or failed for table {}.{}. Falling back to single partition. Error: {}",
                    table_name.schema_name(),
                    table_name.table_name(),
                    e.message()
                );
                false
            }
        }
    }

    async fn write_table_partitions(
        &self,
        block_writer: &mut (dyn BlockWriter + Send),
        table_name: &TableName,
        connection: &dyn JdbcConnection,
    ) -> Result<()> {
        tracing::info!(
            " Total Partition Limit{}",
            SnowflakeConstants::MAX_PARTITION_COUNT
        );
        if self.check_for_view(table_name, connection).await? {
            return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
                    row_num,
                    ALL_PARTITIONS,
                )?;
                Ok(1)
            });
        }
        tracing::info!("{}", SnowflakeConstants::COUNT_RECORDS_QUERY);
        let statement = PreparedStatementBuilder::new()
            .with_connection(connection)
            .with_query(SnowflakeConstants::COUNT_RECORDS_QUERY)?
            .with_parameters(vec![
                table_name.schema_name().to_string(),
                table_name.table_name().to_string(),
            ])?
            .build()
            .await?;
        let mut result_set = connection.execute_query(&statement).await?;
        let mut total_record_count = 0_i64;
        while result_set.next().await? {
            total_record_count = result_set.get_long(1usize)?.unwrap_or(0);
        }
        if total_record_count <= 0 {
            tracing::info!("No Records Found for table {}", table_name);
            return Ok(());
        }
        let primary_key = if total_record_count > SnowflakeConstants::LARGE_TABLE_THRESHOLD {
            tracing::info!(
                "Table {} has {} rows (exceeds threshold {}). Skipping primary key uniqueness check.",
                table_name.table_name(),
                total_record_count,
                SnowflakeConstants::LARGE_TABLE_THRESHOLD
            );
            None
        } else {
            self.get_primary_key(table_name, connection).await?
        };
        for partition_val in partition_values(total_record_count, primary_key.as_deref()) {
            tracing::info!("partitionVal {} ", partition_val);
            block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
                    row_num,
                    partition_val.as_str(),
                )?;
                Ok(1)
            })?;
        }
        Ok(())
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "the partition arithmetic follows the Java double math"
)]
pub fn partition_values(total_record_count: i64, primary_key: Option<&str>) -> Vec<String> {
    let total = total_record_count as f64;
    let records_in_partition =
        (total / f64::from(SnowflakeConstants::MAX_PARTITION_COUNT)).ceil() as i64;
    let partition_record_count = if total_record_count
        <= i64::from(SnowflakeConstants::SINGLE_SPLIT_LIMIT_COUNT)
        || primary_key.is_none()
    {
        total_record_count
    } else {
        records_in_partition
    };
    tracing::info!(" Total Page Count: {}", partition_record_count);
    let number_of_partitions = (total / partition_record_count as f64).ceil() as i32;
    let mut values = Vec::new();
    let mut offset = 0_i64;
    for _ in 1..=number_of_partitions {
        values.push(format!(
            "{}{}{}{}{}{}{}",
            SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
            SnowflakeConstants::PARTITION_PRIMARY_MARKER,
            primary_key.unwrap_or(""),
            SnowflakeConstants::PARTITION_LIMIT_MARKER,
            partition_record_count,
            SnowflakeConstants::PARTITION_OFFSET_MARKER,
            offset
        ));
        offset += partition_record_count;
    }
    values
}

fn fill_template(template: &str, args: &[&str]) -> String {
    let mut parts = template.split("%s");
    let mut filled = parts.next().unwrap_or_default().to_string();
    for (arg, part) in args.iter().zip(parts) {
        filled.push_str(arg);
        filled.push_str(part);
    }
    filled
}

fn java_split_count(text: &str, separator: &str) -> usize {
    let mut parts: Vec<&str> = text.split(separator).collect();
    while parts.len() > 1 && parts.last().is_some_and(|part| part.is_empty()) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() {
        return 1;
    }
    parts.len()
}

fn capabilities(
    config_options: &HashMap<String, String>,
) -> Result<IndexMap<String, Vec<OptimizationSubType>>> {
    let mut capabilities = IndexMap::new();
    let (name, sub_types) = DataSourceOptimizations::SupportsFilterPushdown
        .with_supported_sub_types(&[
            PushdownSubTypes::from(FilterPushdownSubType::SortedRangeSet),
            PushdownSubTypes::from(FilterPushdownSubType::NullableComparison),
        ])?;
    capabilities.insert(name, sub_types);
    let (name, sub_types) = DataSourceOptimizations::SupportsLimitPushdown
        .with_supported_sub_types(&[PushdownSubTypes::from(
            LimitPushdownSubType::IntegerConstant,
        )])?;
    capabilities.insert(name, sub_types);
    let function_names: Vec<String> = StandardFunctions::values()
        .iter()
        .map(|f| f.function_name().function_name().to_string())
        .collect();
    let function_names: Vec<&str> = function_names.iter().map(String::as_str).collect();
    let (name, sub_types) = DataSourceOptimizations::SupportsComplexExpressionPushdown
        .with_supported_sub_types(&[PushdownSubTypes::from(
            ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes
                .with_sub_type_properties(&function_names)?,
        )])?;
    capabilities.insert(name, sub_types);
    let (name, sub_types) = DataSourceOptimizations::SupportsTopNPushdown
        .with_supported_sub_types(&[PushdownSubTypes::from(
            TopNPushdownSubType::SupportsOrderBy,
        )])?;
    capabilities.insert(name, sub_types);
    JDBC_QUERY_PASSTHROUGH
        .add_query_passthrough_capability_if_enabled(&mut capabilities, config_options);
    Ok(capabilities)
}

#[async_trait]
impl FederationRequestHandler for SnowflakeMetadataHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }

    async fn get_credential_provider_with_request_override_configuration(
        &self,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        Ok(self
            .create_snowflake_credentials_provider(
                self.database_connection_secret(),
                request_override_configuration,
            )
            .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>))
    }

    async fn create_credentials_provider(
        &self,
        secret_name: &str,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Box<dyn CredentialsProvider>> {
        self.create_snowflake_credentials_provider(
            Some(secret_name),
            request_override_configuration,
        )
        .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>)
        .ok_or_else(|| {
            FederationError::null_pointer("credentials provider is null for a blank secret name")
        })
    }
}

#[async_trait]
impl MetadataHandler for SnowflakeMetadataHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        self.base.metadata_base()
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.jdbc_do_list_schema_names(allocator, request).await
    }

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.jdbc_do_list_tables(allocator, request).await
    }

    async fn do_get_query_passthrough_schema(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.jdbc_do_get_query_passthrough_schema(allocator, request)
            .await
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.jdbc_do_get_table(allocator, request).await
    }

    fn enhance_partition_schema(
        &self,
        partition_schema_builder: &mut SchemaBuilder,
        request: &GetTableLayoutRequest,
    ) {
        let s3_export_enabled = self.s3_export_enabled();
        if request.constraints().is_query_pass_through() && !s3_export_enabled {
            return;
        }
        if partition_schema_builder
            .get_field(SnowflakeConstants::S3_ENHANCED_PARTITION_COLUMN_NAME)
            .is_none()
            && s3_export_enabled
        {
            tracing::info!(
                "enhancePartitionSchema for S3 export {}: Catalog {}, table {}",
                request.query_id().unwrap_or("null"),
                request.table_name().schema_name(),
                request.table_name()
            );
            partition_schema_builder.add_field_with_type(
                SnowflakeConstants::S3_ENHANCED_PARTITION_COLUMN_NAME,
                DataType::Binary,
            );
        } else if partition_schema_builder
            .get_field(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME)
            .is_none()
        {
            partition_schema_builder.add_field_with_type(
                SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
                DataType::Utf8,
            );
        }
    }

    async fn get_partitions(
        &self,
        block_writer: &mut (dyn BlockWriter + Send),
        request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        let mut table_name = request.table_name().clone();
        let uppercase_only = identity_option(
            request.identity(),
            EnvironmentConstants::CATALOG_CASING_FILTER,
        )
        .is_some_and(|value| value.as_deref() == Some(EnvironmentConstants::UPPERCASE_ONLY));
        if uppercase_only {
            table_name = TableName::new(
                table_name.schema_name().to_uppercase(),
                table_name.table_name().to_uppercase(),
            );
        }
        tracing::debug!(
            "getPartitions: {}: Schema {}, table {}",
            request.query_id().unwrap_or("null"),
            table_name.schema_name(),
            table_name.table_name()
        );
        if self.s3_export_enabled() {
            let serialized = schema_to_bytes(request.schema(), IpcFormat::ArrowDefault)?;
            return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    SnowflakeConstants::S3_ENHANCED_PARTITION_COLUMN_NAME,
                    row_num,
                    serialized.clone(),
                )?;
                Ok(1)
            });
        }
        let connection = self.jdbc_get_connection(request.identity()).await?;
        let result = self
            .write_table_partitions(block_writer, &table_name, connection.as_ref())
            .await;
        close_after(connection.as_ref(), result).await
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        if self.s3_export_enabled() {
            self.handle_s3_export_splits(request).await
        } else {
            self.handle_direct_query_splits(request).await
        }
    }

    async fn do_get_data_source_capabilities(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        tracing::debug!("doGetDataSourceCapabilities: {:?}", request.catalog_name());
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            capabilities(self.config_options())?,
        ))
    }
}

#[async_trait]
impl JdbcMetadataHandler for SnowflakeMetadataHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    async fn list_database_names(
        &self,
        jdbc_connection: &dyn JdbcConnection,
    ) -> Result<Vec<String>> {
        let catalog = jdbc_connection.get_catalog().await?;
        let mut result_set = jdbc_connection
            .metadata()
            .get_schemas_with_catalog(catalog.as_deref(), None)
            .await?;
        let input_catalog_name = jdbc_connection.get_catalog().await?;
        let input_schema_name = jdbc_connection.get_schema().await?;
        let mut schema_names: Vec<String> = Vec::new();
        while result_set.next().await? {
            let schema_name = result_set.get_string("TABLE_SCHEM")?.ok_or_else(|| {
                FederationError::null_pointer(
                    "Cannot invoke \"String.equals(Object)\" because \"schemaName\" is null",
                )
            })?;
            let catalog_name = result_set.get_string("TABLE_CATALOG")?;
            let schema_matches = input_schema_name
                .as_deref()
                .is_none_or(|input| schema_name == input);
            let should_add_schema =
                schema_matches && schema_name != "information_schema" && match &catalog_name {
                    Some(name) => input_catalog_name.as_deref() == Some(name.as_str()),
                    None => return Err(FederationError::null_pointer(
                        "Cannot invoke \"String.equals(Object)\" because \"catalogName\" is null",
                    )),
                };
            if should_add_schema && !schema_names.contains(&schema_name) {
                schema_names.push(schema_name);
            }
        }
        Ok(schema_names)
    }

    async fn list_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        list_tables_request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        tracing::debug!("Starting listPaginatedTables for Snowflake.");
        let mut page_size = list_tables_request.page_size();
        let token = PaginationHelper::validate_and_parse_pagination_arguments(
            list_tables_request.next_token(),
            page_size,
        )?;
        if page_size == UNLIMITED_PAGE_SIZE_VALUE {
            page_size = i32::MAX;
        }
        let adjusted_schema_name = self
            .base
            .case_resolver()
            .get_adjusted_schema_name_string(
                connection,
                list_tables_request.schema_name().unwrap_or_default(),
                self.config_options(),
            )
            .await?;
        tracing::info!(
            "Starting pagination at {} with page size {}",
            token,
            page_size
        );
        let paginated_tables = self
            .get_paginated_tables(connection, &adjusted_schema_name, token, page_size)
            .await?;
        let next_token =
            PaginationHelper::calculate_next_token(token, page_size, &paginated_tables);
        tracing::info!(
            "{} tables returned. Next token is {:?}",
            paginated_tables.len(),
            next_token
        );
        Ok(ListTablesResponse::new(
            list_tables_request.catalog_name(),
            paginated_tables,
            next_token,
        ))
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
        SnowflakeArrowTypeConverter::to_arrow_type(column_type, precision, scale, config_options)
    }

    async fn get_schema(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
        _request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Schema> {
        tracing::debug!("getSchema start, tableName:{}", table_name);
        let built = self
            .build_schema(jdbc_connection, table_name, partition_schema)
            .await;
        match built {
            Err(error) => match snowflake_sql_exception(&error) {
                Some(sql) => Err(FederationError::connector(
                    FederationSourceErrorCode::AccessDeniedException,
                    sql.message(),
                )),
                None => Err(error),
            },
            ok => ok,
        }
    }

    fn get_partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        if self.s3_export_enabled() {
            tracing::debug!("Skipping partition, s3 export enable: {}", catalog_name);
            return Ok(schema_builder.build());
        }
        tracing::debug!("getPartitionSchema: {}", catalog_name);
        schema_builder.add_field_with_type(
            SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME,
            DataType::Utf8,
        );
        Ok(schema_builder.build())
    }
}

impl SnowflakeMetadataHandler {
    async fn build_schema(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        let catalog = jdbc_connection.get_catalog().await?;
        let mut result_set = self
            .get_columns(catalog.as_deref(), table_name, jdbc_connection.metadata())
            .await?;
        let mut found = false;
        while result_set.next().await? {
            found = true;
            let column_type = SnowflakeArrowTypeConverter::to_arrow_type(
                result_set.get_int("DATA_TYPE")?.unwrap_or(0),
                result_set.get_int("COLUMN_SIZE")?.unwrap_or(0),
                result_set.get_int("DECIMAL_DIGITS")?.unwrap_or(0),
                self.config_options(),
            )?;
            let column_name = result_set
                .get_string(COLUMN_NAME)?
                .unwrap_or_else(|| "null".to_string());
            let column_type = match column_type {
                None => DataType::Utf8,
                Some(arrow_type) if !SupportedTypes::is_supported_arrow_type(&arrow_type) => {
                    tracing::warn!(
                        "getSchema: Unable to map type for column[{}] to a supported type, attempted {:?}",
                        column_name,
                        arrow_type
                    );
                    DataType::Utf8
                }
                Some(arrow_type) => arrow_type,
            };
            tracing::debug!(
                " AddField Schema Building... name:{}, type:{:?} ",
                column_name,
                column_type
            );
            schema_builder.add_field(FieldBuilder::new_builder(&column_name, column_type).build());
        }
        if !found {
            return Err(FederationError::connector(
                FederationSourceErrorCode::EntityNotFoundException,
                format!("Could not find table in {}", table_name.schema_name()),
            ));
        }
        for field in partition_schema.fields() {
            schema_builder.add_field(Arc::clone(field));
        }
        Ok(schema_builder.build())
    }
}
