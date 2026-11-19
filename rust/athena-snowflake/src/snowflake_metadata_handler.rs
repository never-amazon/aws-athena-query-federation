use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::credentials::CredentialsProvider;
use athena_federation_sdk::data::{
    ArrowType, Block, FieldBuilder, SchemaBuilder, SimpleBlockWriter, SupportedTypes,
};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{
    AwsClients, HandlerContext, MetadataContext, MetadataHandler,
};
use athena_federation_sdk::metadata::optimizations::pushdown::{
    LimitPushdownSubType, TopNPushdownSubType,
};
use athena_federation_sdk::metadata::optimizations::{Capability, OptimizationSubType};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::security::AwsRequestOverrideConfiguration;
use athena_federation_sdk::serde::{schema_from_bytes, schema_to_bytes, IpcFormat};
use athena_federation_sdk::util::{
    calculate_next_token, validate_and_parse_pagination_arguments, UNLIMITED_PAGE_SIZE_VALUE,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::close_after;
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, JdbcConnectionFactory,
};
use athena_jdbc::manager::{
    columns, complex_expression_capability, filter_capability, jdbc_util, ConnectionSource,
    JdbcMetadataHandler, MetadataDialect, PreparedStatementBuilder, TABLES_AND_VIEWS,
};
use athena_jdbc::qpt::{JDBC_QUERY_PASSTHROUGH, QUERY};
use athena_jdbc::resolver::{JdbcCaseResolver, JdbcCaseResolverExt};
use athena_jdbc::sql::{JdbcConnection, ResultSetMetaData, SqlException};
use indexmap::IndexMap;

use crate::connection::SnowflakeConnectionFactory;
use crate::resolver::SnowflakeJdbcCaseResolver;
use crate::s3_export::{S3ObjectLister, S3ObjectSummary, S3Uri};
use crate::snowflake_constants::{self, is_blank};
use crate::snowflake_credentials_provider::SnowflakeCredentialsProvider;
use crate::snowflake_environment_properties::snowflake_parameters;
use crate::snowflake_query_string_builder::{
    base_export_sql, query_string_builder, single_quote, SnowflakeQueryStringBuilder,
};
use crate::sql::SNOWFLAKE_SQL_EXCEPTION;
use crate::utils;

const MAX_SPLITS_PER_REQUEST: usize = 1_000_000;
const COLUMN_NAME: &str = "COLUMN_NAME";
const COUNTS_COLUMN_NAME: &str = "COUNTS";
const PRIMARY_KEY_COLUMN_NAME: &str = "column_name";
const ALL_PARTITIONS: &str = "*";
const FILES_ALREADY_EXISTING: &str = "Files already existing";
const INTEGRATION_NOT_FOUND: &str = "does not exist or not authorized";
const MISSING_S3_CLIENT: &str =
    "Cannot invoke \"software.amazon.awssdk.services.s3.S3Client.utilities()\" because \"this.amazonS3\" is null";

pub struct SnowflakeMetadataDialect {
    amazon_s3: Option<Arc<dyn S3ObjectLister>>,
    snowflake_query_string_builder: SnowflakeQueryStringBuilder,
}

pub type SnowflakeMetadataHandler = JdbcMetadataHandler<SnowflakeMetadataDialect>;

pub(crate) fn java_text(error: &FederationError) -> String {
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

impl SnowflakeMetadataDialect {
    pub fn connection_factory(
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<SnowflakeConnectionFactory> {
        SnowflakeConnectionFactory::new_with_config_options(
            config.clone(),
            Some(snowflake_parameters(
                &snowflake_constants::jdbc_properties(),
                config_options,
            )),
            DatabaseConnectionInfo::new(
                snowflake_constants::SNOWFLAKE_DRIVER_CLASS,
                snowflake_constants::SNOWFLAKE_DEFAULT_PORT,
            )?,
            Some(config_options),
        )
    }
}

impl SnowflakeMetadataDialect {
    pub fn get_sf_storage_integration_name_from_config(
        config_options: &HashMap<String, String>,
    ) -> Option<&str> {
        config_options
            .get(snowflake_constants::STORAGE_INTEGRATION_CONFIG_KEY)
            .map(String::as_str)
    }
}

fn storage_integration_name(config_options: &HashMap<String, String>) -> Result<String> {
    SnowflakeMetadataDialect::get_sf_storage_integration_name_from_config(config_options)
        .map(ToString::to_string)
        .ok_or_else(|| {
            FederationError::connector(
                FederationSourceErrorCode::InvalidInputException,
                "Snowflake storage integration name not found",
            )
        })
}

pub fn create_snowflake_credentials_provider(
    context: &HandlerContext,
    secret_name: Option<&str>,
    request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
) -> Option<SnowflakeCredentialsProvider> {
    let secret_name = secret_name.filter(|name| !is_blank(Some(name)))?;
    Some(SnowflakeCredentialsProvider::new(
        secret_name,
        Arc::clone(context.secrets_manager().secrets_manager()),
        request_override_configuration.cloned(),
    ))
}

pub fn create_credentials_provider(
    context: &HandlerContext,
    secret_name: &str,
    request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
) -> Result<Box<dyn CredentialsProvider>> {
    create_snowflake_credentials_provider(
        context,
        Some(secret_name),
        request_override_configuration,
    )
    .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>)
    .ok_or_else(|| {
        FederationError::null_pointer("credentials provider is null for a blank secret name")
    })
}

impl SnowflakeMetadataDialect {
    pub fn new(amazon_s3: Option<Arc<dyn S3ObjectLister>>) -> SnowflakeMetadataDialect {
        SnowflakeMetadataDialect {
            amazon_s3,
            snowflake_query_string_builder: query_string_builder(),
        }
    }

    pub fn handler(
        context: MetadataContext,
        config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
        case_resolver: Arc<dyn JdbcCaseResolver>,
        amazon_s3: Option<Arc<dyn S3ObjectLister>>,
    ) -> SnowflakeMetadataHandler {
        JdbcMetadataHandler::new(
            context,
            ConnectionSource::Direct {
                config,
                factory: connection_factory,
            },
            case_resolver,
            SnowflakeMetadataDialect::new(amazon_s3),
        )
    }

    pub fn from_config(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
        amazon_s3: Option<Arc<dyn S3ObjectLister>>,
    ) -> Result<SnowflakeMetadataHandler> {
        let factory = Arc::new(SnowflakeMetadataDialect::connection_factory(
            &config,
            &config_options,
        )?);
        let context = MetadataContext::builder(config.engine(), clients)
            .config_options(config_options)
            .build()?;
        Ok(SnowflakeMetadataDialect::handler(
            context,
            config,
            factory,
            Arc::new(SnowflakeJdbcCaseResolver::new(
                snowflake_constants::SNOWFLAKE_NAME,
            )),
            amazon_s3,
        ))
    }

    pub async fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<SnowflakeMetadataHandler> {
        let config = jdbc_util::single_database_config_from_env(
            snowflake_constants::SNOWFLAKE_NAME,
            &config_options,
        )?;
        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        SnowflakeMetadataDialect::from_config(
            clients,
            config,
            config_options,
            Some(Arc::new(aws_sdk_s3::Client::new(&sdk_config))),
        )
    }

    pub async fn get_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        database_name: &str,
        offset: i32,
        limit: i32,
    ) -> std::result::Result<Vec<TableName>, SqlException> {
        let mut statement = connection
            .prepare_statement(snowflake_constants::LIST_PAGINATED_TABLES_QUERY)
            .await?;
        statement.set_string(1, database_name)?;
        statement.set_int(2, limit)?;
        statement.set_int(3, offset)?;
        jdbc_util::table_metadata(connection, &statement, TABLES_AND_VIEWS).await
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
        let check_integration_query = snowflake_constants::DESCRIBE_STORAGE_INTEGRATION_TEMPLATE
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
                        .get_string(snowflake_constants::STORAGE_INTEGRATION_PROPERTY_KEY)?
                        .unwrap_or_else(|| "null".to_string()),
                    result_set
                        .get_string(snowflake_constants::STORAGE_INTEGRATION_PROPERTY_VALUE_KEY)?
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

    pub fn validate_sf_storage_integration_exist_and_valid(
        storage_integration_map: &HashMap<String, String>,
    ) -> Result<()> {
        let bucket_key = snowflake_constants::STORAGE_INTEGRATION_BUCKET_KEY;
        let provider_key = snowflake_constants::STORAGE_INTEGRATION_STORAGE_PROVIDER_KEY;
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
        SnowflakeMetadataDialect::validate_sf_storage_integration_exist_and_valid(&properties)?;
        let mut bucket_path = properties
            .get(snowflake_constants::STORAGE_INTEGRATION_BUCKET_KEY)
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
            .get_field_reader(snowflake_constants::S3_ENHANCED_PARTITION_COLUMN_NAME)
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
        let generated = base_export_sql(
            &self.snowflake_query_string_builder,
            request.table_name(),
            &schema,
            constraints,
        )?;
        Ok(generated.unwrap_or_else(|| "null".to_string()))
    }

    async fn handle_direct_query_splits(
        &self,
        handler: &SnowflakeMetadataHandler,
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
            return handler.query_passthrough_split(request).await;
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
                    .get_field_reader(snowflake_constants::BLOCK_PARTITION_COLUMN_NAME)
                    .ok_or_else(missing_field_reader)?;
                location_reader.set_position(cur_partition);
                location_reader.read_text()
            };
            let mut builder = handler.context().split_builder(request).await?;
            tracing::info!(
                "{}: Input partition is {:?}",
                request.query_id().unwrap_or("null"),
                partition
            );
            builder = builder.add(
                snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
                partition.unwrap_or_else(|| "null".to_string()),
            );
            if let Some(filter) = identity_option(
                request.identity(),
                environment_constants::CATALOG_CASING_FILTER,
            ) {
                tracing::info!("Catalog Casing Filter found: {:?}", filter);
                builder = builder.add(
                    environment_constants::CATALOG_CASING_FILTER,
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
        handler: &SnowflakeMetadataHandler,
        request: &GetSplitsRequest,
        s3_uri: &mut Option<S3Uri>,
    ) -> Result<()> {
        let query_id = request.query_id().unwrap_or("null");
        let connection = handler.connect(request.identity()).await?;
        let result = async {
            let sf_integration_name = storage_integration_name(handler.config_options())?;
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
                snowflake_constants::COPY_INTO_QUERY_TEMPLATE,
                &[
                    &single_quote(&s3_path),
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
        handler: &SnowflakeMetadataHandler,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        let query_id = request.query_id().unwrap_or("null");
        let mut s3_uri = None;
        if let Err(error) = self.run_export(handler, request, &mut s3_uri).await {
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
            splits.push(export_split(handler, request, bucket, "").await?);
        } else {
            for object_summary in &s3_object_summaries {
                splits.push(export_split(handler, request, bucket, object_summary.key()).await?);
            }
        }
        Ok(GetSplitsResponse::new(request.catalog_name(), splits, None))
    }

    async fn check_for_view(
        &self,
        table_name: &TableName,
        connection: &dyn JdbcConnection,
    ) -> Result<bool> {
        let statement = PreparedStatementBuilder::new()
            .with_connection(connection)
            .with_query(snowflake_constants::VIEW_CHECK_QUERY)?
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
            snowflake_constants::SHOW_PRIMARY_KEYS_QUERY,
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
            statement.set_query_timeout(snowflake_constants::UNIQUENESS_CHECK_TIMEOUT_SECONDS)?;
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
        block_writer: &mut SimpleBlockWriter<'_>,
        table_name: &TableName,
        connection: &dyn JdbcConnection,
    ) -> Result<()> {
        tracing::info!(
            " Total Partition Limit{}",
            snowflake_constants::MAX_PARTITION_COUNT
        );
        if self.check_for_view(table_name, connection).await? {
            return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
                    row_num,
                    ALL_PARTITIONS,
                )?;
                Ok(1)
            });
        }
        tracing::info!("{}", snowflake_constants::COUNT_RECORDS_QUERY);
        let statement = PreparedStatementBuilder::new()
            .with_connection(connection)
            .with_query(snowflake_constants::COUNT_RECORDS_QUERY)?
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
        let primary_key = if total_record_count > snowflake_constants::LARGE_TABLE_THRESHOLD {
            tracing::info!(
                "Table {} has {} rows (exceeds threshold {}). Skipping primary key uniqueness check.",
                table_name.table_name(),
                total_record_count,
                snowflake_constants::LARGE_TABLE_THRESHOLD
            );
            None
        } else {
            self.get_primary_key(table_name, connection).await?
        };
        for partition_val in partition_values(total_record_count, primary_key.as_deref()) {
            tracing::info!("partitionVal {} ", partition_val);
            block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
                    row_num,
                    partition_val.as_str(),
                )?;
                Ok(1)
            })?;
        }
        Ok(())
    }
}

async fn export_split(
    handler: &SnowflakeMetadataHandler,
    request: &GetSplitsRequest,
    bucket: &str,
    key: &str,
) -> Result<Split> {
    Ok(handler
        .context()
        .split_builder(request)
        .await?
        .add(
            snowflake_constants::SNOWFLAKE_SPLIT_QUERY_ID,
            request.query_id().unwrap_or("null"),
        )
        .add(snowflake_constants::SNOWFLAKE_SPLIT_EXPORT_BUCKET, bucket)
        .add(snowflake_constants::SNOWFLAKE_SPLIT_OBJECT_KEY, key)
        .build())
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "the partition arithmetic follows the Java double math"
)]
pub fn partition_values(total_record_count: i64, primary_key: Option<&str>) -> Vec<String> {
    let total = total_record_count as f64;
    let records_in_partition =
        (total / f64::from(snowflake_constants::MAX_PARTITION_COUNT)).ceil() as i64;
    let partition_record_count = if total_record_count
        <= i64::from(snowflake_constants::SINGLE_SPLIT_LIMIT_COUNT)
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
            snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
            snowflake_constants::PARTITION_PRIMARY_MARKER,
            primary_key.unwrap_or(""),
            snowflake_constants::PARTITION_LIMIT_MARKER,
            partition_record_count,
            snowflake_constants::PARTITION_OFFSET_MARKER,
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
    let (name, sub_types) = filter_capability().into_entry();
    capabilities.insert(name, sub_types);
    let (name, sub_types) =
        Capability::Limit(vec![LimitPushdownSubType::IntegerConstant]).into_entry();
    capabilities.insert(name, sub_types);
    let (name, sub_types) = complex_expression_capability()?.into_entry();
    capabilities.insert(name, sub_types);
    let (name, sub_types) =
        Capability::TopN(vec![TopNPushdownSubType::SupportsOrderBy]).into_entry();
    capabilities.insert(name, sub_types);
    JDBC_QUERY_PASSTHROUGH
        .add_query_passthrough_capability_if_enabled(&mut capabilities, config_options);
    Ok(capabilities)
}

#[async_trait]
impl MetadataDialect for SnowflakeMetadataDialect {
    fn enhance_partition_schema(
        &self,
        handler: &SnowflakeMetadataHandler,
        partition_schema_builder: &mut SchemaBuilder,
        request: &GetTableLayoutRequest,
    ) {
        let s3_export_enabled = snowflake_constants::is_s3_export_enabled(handler.config_options());
        if request.constraints().is_query_pass_through() && !s3_export_enabled {
            return;
        }
        if partition_schema_builder
            .get_field(snowflake_constants::S3_ENHANCED_PARTITION_COLUMN_NAME)
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
                snowflake_constants::S3_ENHANCED_PARTITION_COLUMN_NAME,
                DataType::Binary,
            );
        } else if partition_schema_builder
            .get_field(snowflake_constants::BLOCK_PARTITION_COLUMN_NAME)
            .is_none()
        {
            partition_schema_builder.add_field_with_type(
                snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
                DataType::Utf8,
            );
        }
    }

    async fn get_partitions(
        &self,
        handler: &SnowflakeMetadataHandler,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        let mut table_name = request.table_name().clone();
        let uppercase_only = identity_option(
            request.identity(),
            environment_constants::CATALOG_CASING_FILTER,
        )
        .is_some_and(|value| value.as_deref() == Some(environment_constants::UPPERCASE_ONLY));
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
        if snowflake_constants::is_s3_export_enabled(handler.config_options()) {
            let serialized = schema_to_bytes(request.schema(), IpcFormat::ArrowDefault)?;
            return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                block.set_value(
                    snowflake_constants::S3_ENHANCED_PARTITION_COLUMN_NAME,
                    row_num,
                    serialized.clone(),
                )?;
                Ok(1)
            });
        }
        let connection = handler.connect(request.identity()).await?;
        let result = self
            .write_table_partitions(block_writer, &table_name, connection.as_ref())
            .await;
        close_after(connection.as_ref(), result).await
    }

    async fn get_splits(
        &self,
        handler: &SnowflakeMetadataHandler,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        if snowflake_constants::is_s3_export_enabled(handler.config_options()) {
            self.handle_s3_export_splits(handler, request).await
        } else {
            self.handle_direct_query_splits(handler, request).await
        }
    }

    fn capabilities(
        &self,
        handler: &SnowflakeMetadataHandler,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        tracing::debug!("doGetDataSourceCapabilities: {:?}", request.catalog_name());
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            capabilities(handler.config_options())?,
        ))
    }

    async fn credentials_provider(
        &self,
        context: &HandlerContext,
        secret_name: Option<&str>,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        Ok(
            create_snowflake_credentials_provider(context, secret_name, override_config)
                .map(|provider| Box::new(provider) as Box<dyn CredentialsProvider>),
        )
    }

    async fn list_database_names(
        &self,
        _handler: &SnowflakeMetadataHandler,
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
        handler: &SnowflakeMetadataHandler,
        connection: &dyn JdbcConnection,
        list_tables_request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        tracing::debug!("Starting listPaginatedTables for Snowflake.");
        let mut page_size = list_tables_request.page_size();
        let token =
            validate_and_parse_pagination_arguments(list_tables_request.next_token(), page_size)?;
        if page_size == UNLIMITED_PAGE_SIZE_VALUE {
            page_size = i32::MAX;
        }
        let adjusted_schema_name = handler
            .case_resolver()
            .get_adjusted_schema_name_string(
                connection,
                list_tables_request.schema_name().unwrap_or_default(),
                handler.config_options(),
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
        let next_token = calculate_next_token(token, page_size, &paginated_tables);
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

    fn convert_datasource_type(
        &self,
        column_index: usize,
        precision: i32,
        config_options: &HashMap<String, String>,
        metadata: &ResultSetMetaData,
    ) -> Result<Option<ArrowType>> {
        let scale = metadata.scale(column_index)?;
        let column_type = metadata.column_type(column_index)?;
        utils::to_arrow_type(column_type, precision, scale, config_options)
    }

    async fn get_schema(
        &self,
        handler: &SnowflakeMetadataHandler,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        tracing::debug!("getSchema start, tableName:{}", table_name);
        let built = self
            .build_schema(handler, jdbc_connection, table_name, partition_schema)
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

    fn partition_schema(
        &self,
        handler: &SnowflakeMetadataHandler,
        catalog_name: &str,
    ) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        if snowflake_constants::is_s3_export_enabled(handler.config_options()) {
            tracing::debug!("Skipping partition, s3 export enable: {}", catalog_name);
            return Ok(schema_builder.build());
        }
        tracing::debug!("getPartitionSchema: {}", catalog_name);
        schema_builder.add_field_with_type(
            snowflake_constants::BLOCK_PARTITION_COLUMN_NAME,
            DataType::Utf8,
        );
        Ok(schema_builder.build())
    }
}

impl SnowflakeMetadataDialect {
    async fn build_schema(
        &self,
        handler: &SnowflakeMetadataHandler,
        jdbc_connection: &dyn JdbcConnection,
        table_name: &TableName,
        partition_schema: &Schema,
    ) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        let catalog = jdbc_connection.get_catalog().await?;
        let mut result_set =
            columns(catalog.as_deref(), table_name, jdbc_connection.metadata()).await?;
        let mut found = false;
        while result_set.next().await? {
            found = true;
            let column_type = utils::to_arrow_type(
                result_set.get_int("DATA_TYPE")?.unwrap_or(0),
                result_set.get_int("COLUMN_SIZE")?.unwrap_or(0),
                result_set.get_int("DECIMAL_DIGITS")?.unwrap_or(0),
                handler.config_options(),
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
