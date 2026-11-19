use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{
    ArrowType, Block, BlockAllocator, SchemaBuilder, SimpleBlockWriter,
};
use athena_federation_sdk::domain::predicate::functions::StandardFunctions;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    AwsClients, FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
};
use athena_federation_sdk::metadata::optimizations::pushdown::{
    ComplexExpressionPushdownSubType, FilterPushdownSubType, HintsSubtype,
};
use athena_federation_sdk::metadata::optimizations::{Capability, OptimizationSubType};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, GenericJdbcConnectionFactory,
    JdbcConnectionFactory, JdbcProperties,
};
use athena_jdbc::manager::{
    JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerExt, JdbcUtil,
    PreparedStatementBuilder, TABLES_AND_VIEWS,
};
use athena_jdbc::resolver::JdbcCaseResolver;
use athena_jdbc::splits::SplitterFactory;
use athena_jdbc::sql::{JdbcConnection, SqlException, SqlValue};
use indexmap::IndexMap;

use crate::constants::PostGreSqlConstants;
use crate::driver::PgDriver;
use crate::resolver::PostGreSqlJdbcCaseResolver;

const MATERIALIZED_VIEWS: &str = "Materialized Views";
const PARTITION_SCHEMA_NAME: &str = "child_schema";
const PARTITION_NAME: &str = "child";
const MAX_SPLITS_PER_REQUEST: usize = 1_000_000;
const GET_DATA_TYPE_QUERY: &str = "SELECT data_type FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND column_name = ?";
const MATERIALIZED_VIEWS_QUERY: &str = "select matviewname as \"TABLE_NAME\", schemaname as \"TABLE_SCHEM\" from pg_catalog.pg_matviews mv where has_table_privilege(format('%I.%I', mv.schemaname, mv.matviewname), 'select') and schemaname = ?";
const CHAR_COLUMNS_QUERY: &str = "SELECT column_name FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND data_type = 'character'";
const NUL_IDENTIFIER_MESSAGE: &str = "Zero bytes may not occur in identifiers.";

pub struct PostGreSqlMetadataHandler {
    base: JdbcMetadataHandlerBase,
    splitter_factory: SplitterFactory,
}

fn number_format(text: &str) -> FederationError {
    FederationError::NumberFormat(format!("For input string: \"{text}\""))
}

fn parse_int(text: &str) -> Result<i32> {
    text.parse::<i32>().map_err(|_| number_format(text))
}

fn missing_field_reader() -> FederationError {
    FederationError::null_pointer(
        "Cannot invoke \"org.apache.arrow.vector.complex.reader.FieldReader.setPosition(int)\" because \"partitionsSchemaFieldReader\" is null",
    )
}

fn partition_text(partitions: &Block, field: &str, row: usize) -> Result<Option<String>> {
    let reader = partitions
        .get_field_reader(field)
        .ok_or_else(missing_field_reader)?;
    reader.set_position(row);
    Ok(reader.read_text())
}

fn java_string(value: Option<String>) -> String {
    value.unwrap_or_else(|| "null".to_string())
}

fn catalog_casing_filter(identity: Option<&FederatedIdentity>) -> Option<String> {
    let value = identity?
        .config_options()
        .get(environment_constants::CATALOG_CASING_FILTER)?;
    tracing::info!("Catalog Casing Filter found: {:?}", value);
    Some(java_string(value.clone()))
}

impl PostGreSqlMetadataHandler {
    pub const JDBC_PROPERTIES: [(&'static str, &'static str); 1] = [("databaseTerm", "SCHEMA")];
    pub const GET_PARTITIONS_QUERY: &'static str = "SELECT nmsp_child.nspname AS child_schema, child.relname AS child FROM pg_inherits JOIN pg_class parent ON pg_inherits.inhparent = parent.oid JOIN pg_class child ON pg_inherits.inhrelid = child.oid JOIN pg_namespace nmsp_parent ON nmsp_parent.oid = parent.relnamespace JOIN pg_namespace nmsp_child ON nmsp_child.oid = child.relnamespace where nmsp_parent.nspname = ? AND parent.relname = ?";
    pub const BLOCK_PARTITION_COLUMN_NAME: &'static str = "partition_name";
    pub const BLOCK_PARTITION_SCHEMA_COLUMN_NAME: &'static str = "partition_schema_name";
    pub const ALL_PARTITIONS: &'static str = "*";
    pub const LIST_PAGINATED_TABLES_QUERY: &'static str = "SELECT a.\"TABLE_NAME\", a.\"TABLE_SCHEM\" FROM ((SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ?) UNION (SELECT matviewname as \"TABLE_NAME\", schemaname as \"TABLE_SCHEM\" from pg_catalog.pg_matviews mv where has_table_privilege(format('%I.%I', mv.schemaname, mv.matviewname), 'select') and schemaname = ?)) AS a ORDER BY a.\"TABLE_NAME\" LIMIT ? OFFSET ?";
    pub const NON_DEFAULT_COLLATE: &'static str = "non_default_collate";
    pub const SQL_SPLITS_STRING: &'static str = "select min(%s), max(%s) from %s.%s";
    pub const DEFAULT_NUM_SPLITS: i32 = 20;

    pub fn jdbc_properties() -> JdbcProperties {
        PostGreSqlMetadataHandler::JDBC_PROPERTIES
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    pub fn connection_factory(
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<GenericJdbcConnectionFactory> {
        GenericJdbcConnectionFactory::new_with_config_options(
            config.clone(),
            Some(PostGreSqlMetadataHandler::jdbc_properties()),
            DatabaseConnectionInfo::new(
                PostGreSqlConstants::POSTGRESQL_DRIVER_CLASS,
                PostGreSqlConstants::POSTGRESQL_DEFAULT_PORT,
            )?,
            Some(config_options),
            Arc::new(PgDriver),
        )
    }

    pub fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlMetadataHandler> {
        let config = JdbcUtil::get_single_database_config_from_env(
            PostGreSqlConstants::POSTGRES_NAME,
            &config_options,
        )?;
        PostGreSqlMetadataHandler::new(clients, config, config_options)
    }

    pub fn new(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlMetadataHandler> {
        let factory = Arc::new(PostGreSqlMetadataHandler::connection_factory(
            &config,
            &config_options,
        )?);
        let metadata_base = MetadataHandlerBase::builder(config.engine(), clients)
            .config_options(config_options)
            .build()?;
        Ok(PostGreSqlMetadataHandler::with_base(
            JdbcMetadataHandlerBase::new_with_case_resolver(
                metadata_base,
                config,
                factory,
                Arc::new(PostGreSqlJdbcCaseResolver::new(
                    PostGreSqlConstants::POSTGRES_NAME,
                )),
            ),
        ))
    }

    pub fn with_connection_factory(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlMetadataHandler> {
        PostGreSqlMetadataHandler::with_case_resolver(
            clients,
            config,
            jdbc_connection_factory,
            config_options,
            Arc::new(PostGreSqlJdbcCaseResolver::new(
                PostGreSqlConstants::POSTGRES_NAME,
            )),
        )
    }

    pub fn with_case_resolver(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        jdbc_connection_factory: Arc<dyn JdbcConnectionFactory>,
        config_options: HashMap<String, String>,
        case_resolver: Arc<dyn JdbcCaseResolver>,
    ) -> Result<PostGreSqlMetadataHandler> {
        let metadata_base = MetadataHandlerBase::builder(config.engine(), clients)
            .config_options(config_options)
            .encryption_key_factory(None)
            .spill_bucket(None)
            .spill_prefix(None)
            .build()?;
        Ok(PostGreSqlMetadataHandler::with_base(
            JdbcMetadataHandlerBase::new_with_case_resolver(
                metadata_base,
                config,
                jdbc_connection_factory,
                case_resolver,
            ),
        ))
    }

    pub fn with_base(base: JdbcMetadataHandlerBase) -> PostGreSqlMetadataHandler {
        PostGreSqlMetadataHandler {
            base,
            splitter_factory: SplitterFactory::new(),
        }
    }

    pub async fn get_char_columns(
        connection: &dyn JdbcConnection,
        schema: &str,
        table: &str,
    ) -> std::result::Result<Vec<String>, SqlException> {
        let mut statement = connection.prepare_statement(CHAR_COLUMNS_QUERY).await?;
        statement.set_string(1, schema)?;
        statement.set_string(2, table)?;
        let mut result_set = connection.execute_query(&statement).await?;
        let mut char_columns = Vec::new();
        while result_set.next().await? {
            char_columns.push(java_string(result_set.get_string("column_name")?));
        }
        Ok(char_columns)
    }

    pub fn escape_identifier(input: &str) -> Result<String> {
        if input.contains('\0') {
            return Err(FederationError::Runtime(format!(
                "Unable to escape SQL identifier: {NUL_IDENTIFIER_MESSAGE}"
            )));
        }
        Ok(format!("\"{}\"", input.replace('"', "\"\"")))
    }

    pub fn is_valid_split_column(&self, column_name: Option<&str>) -> bool {
        column_name.is_some_and(|name| !name.is_empty() && !name.chars().any(char::is_control))
    }

    async fn get_materialized_views(
        &self,
        connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> std::result::Result<Vec<TableName>, SqlException> {
        let mut statement = connection
            .prepare_statement(MATERIALIZED_VIEWS_QUERY)
            .await?;
        statement.set_string(1, database_name)?;
        tracing::debug!(
            "Prepared Statement for getting Materialized View in schema {} : {}",
            database_name,
            statement.sql()
        );
        JdbcUtil::get_table_metadata(connection, &statement, MATERIALIZED_VIEWS).await
    }

    async fn get_paginated_results(
        &self,
        connection: &dyn JdbcConnection,
        database_name: Option<&str>,
        token: i32,
        limit: i32,
    ) -> std::result::Result<Vec<TableName>, SqlException> {
        let mut statement = connection
            .prepare_statement(PostGreSqlMetadataHandler::LIST_PAGINATED_TABLES_QUERY)
            .await?;
        let name = database_name.map_or(SqlValue::Null, |n| SqlValue::String(n.to_string()));
        statement.set_object(1, name.clone())?;
        statement.set_object(2, name)?;
        statement.set_int(3, limit)?;
        statement.set_int(4, token)?;
        tracing::debug!(
            "Prepared Statement for getting tables in schema {:?} : {}",
            database_name,
            statement.sql()
        );
        JdbcUtil::get_table_metadata(connection, &statement, TABLES_AND_VIEWS).await
    }

    async fn is_uuid_column(
        &self,
        connection: &dyn JdbcConnection,
        table_name: &TableName,
        column_name: &str,
    ) -> bool {
        let result: std::result::Result<bool, SqlException> = async {
            let mut statement = connection.prepare_statement(GET_DATA_TYPE_QUERY).await?;
            statement.set_string(1, table_name.schema_name())?;
            statement.set_string(2, table_name.table_name())?;
            statement.set_string(3, column_name)?;
            let mut result_set = connection.execute_query(&statement).await?;
            if result_set.next().await? {
                let data_type = result_set.get_string("data_type")?;
                tracing::debug!(
                    "Data type for Column: {}.{}.{} is: {:?}",
                    table_name.schema_name(),
                    table_name.table_name(),
                    column_name,
                    data_type
                );
                return Ok(data_type.is_some_and(|t| t.eq_ignore_ascii_case("uuid")));
            }
            Ok(false)
        }
        .await;
        result.unwrap_or_else(|ex| {
            tracing::warn!(
                "Error checking if column {} is UUID type for table {}.{}: {}",
                column_name,
                table_name.schema_name(),
                table_name.table_name(),
                ex
            );
            false
        })
    }

    async fn split_clauses_from(
        &self,
        connection: &dyn JdbcConnection,
        table_name: &TableName,
    ) -> Result<Vec<String>> {
        let mut split_clauses = Vec::new();
        let mut result_set = connection
            .metadata()
            .get_primary_keys(
                None,
                Some(table_name.schema_name()),
                table_name.table_name(),
            )
            .await?;
        let primary_key_column = if result_set.next().await? {
            result_set.get_string("COLUMN_NAME")?
        } else {
            None
        };
        let Some(primary_key_column) = primary_key_column else {
            return Ok(split_clauses);
        };
        if !self.is_valid_split_column(Some(&primary_key_column)) {
            tracing::warn!(
                "Primary key column name is not a valid split identifier for table {}.{}. Skipping split generation.",
                table_name.schema_name(),
                table_name.table_name()
            );
            return Ok(split_clauses);
        }
        if self
            .is_uuid_column(connection, table_name, &primary_key_column)
            .await
        {
            tracing::info!(
                "Primary key '{}' is UUID type. Skipping split generation for table: {}.{} (UUID columns do not support MIN/MAX functions in PostgreSQL)",
                primary_key_column,
                table_name.schema_name(),
                table_name.table_name()
            );
            return Ok(split_clauses);
        }
        let quoted_primary_key_column =
            PostGreSqlMetadataHandler::escape_identifier(&primary_key_column)?;
        let sql = format!(
            "select min({quoted_primary_key_column}), max({quoted_primary_key_column}) from {}.{}",
            PostGreSqlMetadataHandler::escape_identifier(table_name.schema_name())?,
            PostGreSqlMetadataHandler::escape_identifier(table_name.table_name())?
        );
        let statement = connection.prepare_statement(&sql).await?;
        let mut min_max = connection.execute_query(&statement).await?;
        min_max.next().await?;
        let min = min_max.get_long(1usize)?.unwrap_or(0);
        let max = min_max.get_long(2usize)?.unwrap_or(0);
        let splitter = self.splitter_factory.get_splitter(
            &quoted_primary_key_column,
            &min_max,
            PostGreSqlMetadataHandler::DEFAULT_NUM_SPLITS,
        )?;
        if let Some(mut splitter) = splitter {
            if max - min < i64::from(PostGreSqlMetadataHandler::DEFAULT_NUM_SPLITS) {
                tracing::info!(
                    "Range too small for splitting (min={}, max={}), skipping",
                    min,
                    max
                );
                return Ok(split_clauses);
            }
            while splitter.has_next() {
                let split_clause = splitter.next_range_clause();
                tracing::debug!("Split generated {}", split_clause);
                split_clauses.push(split_clause);
            }
        }
        Ok(split_clauses)
    }

    pub async fn get_split_clauses(
        &self,
        table_name: &TableName,
        request: Option<&GetSplitsRequest>,
    ) -> Vec<String> {
        let result = async {
            let connection = self
                .jdbc_get_connection(request.and_then(GetSplitsRequest::identity))
                .await?;
            let clauses = self
                .split_clauses_from(connection.as_ref(), table_name)
                .await;
            let closed = connection.close().await;
            match (clauses, closed) {
                (Err(e), _) => Err(e),
                (Ok(_), Err(e)) => Err(FederationError::from(e)),
                (Ok(clauses), Ok(())) => Ok(clauses),
            }
        }
        .await;
        result.unwrap_or_else(|ex| {
            tracing::warn!("Unable to split data. {}", ex);
            Vec::new()
        })
    }

    async fn make_split(
        &self,
        request: &GetSplitsRequest,
        partition_schema: String,
        partition: String,
    ) -> Result<Split> {
        let spill_location = self.make_spill_location(request)?;
        let override_config = self
            .get_request_override_config_with_config_options(
                request.identity().map(FederatedIdentity::config_options),
            )
            .await?;
        let encryption_key = self
            .make_encryption_key_with_aws_request_override_configuration(override_config.as_ref())
            .await?;
        let mut builder = Split::new_builder(Some(spill_location), encryption_key)
            .add(
                PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                partition_schema,
            )
            .add(
                PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
                partition,
            );
        if let Some(filter) = catalog_casing_filter(request.identity()) {
            builder = builder.add(environment_constants::CATALOG_CASING_FILTER, filter);
        }
        Ok(builder.build())
    }
}

impl FederationRequestHandler for PostGreSqlMetadataHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

fn capabilities(
    config_options: &HashMap<String, String>,
) -> Result<IndexMap<String, Vec<OptimizationSubType>>> {
    let mut capabilities = IndexMap::new();
    let (name, sub_types) = Capability::Filter(vec![
        FilterPushdownSubType::SortedRangeSet,
        FilterPushdownSubType::NullableComparison,
    ])
    .into_entry();
    capabilities.insert(name, sub_types);
    let function_names: Vec<String> = StandardFunctions::values()
        .iter()
        .map(|f| f.function_name().function_name().to_string())
        .collect();
    let function_names: Vec<&str> = function_names.iter().map(String::as_str).collect();
    let (name, sub_types) = Capability::ComplexExpression(vec![
        ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes
            .with_sub_type_properties(&function_names)?,
    ])
    .into_entry();
    capabilities.insert(name, sub_types);
    athena_jdbc::qpt::JDBC_QUERY_PASSTHROUGH
        .add_query_passthrough_capability_if_enabled(&mut capabilities, config_options);
    let non_default_collate = config_options
        .get(PostGreSqlMetadataHandler::NON_DEFAULT_COLLATE)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if non_default_collate {
        let (name, sub_types) =
            Capability::Hints(vec![HintsSubtype::NonDefaultCollate]).into_entry();
        capabilities.insert(name, sub_types);
    }
    Ok(capabilities)
}

#[async_trait]
impl MetadataHandler for PostGreSqlMetadataHandler {
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

    async fn get_partitions(
        &self,
        block_writer: &mut SimpleBlockWriter<'_>,
        request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        tracing::info!(
            "{}: Catalog {}, table {}",
            request.query_id().unwrap_or("null"),
            request.table_name().schema_name(),
            request.table_name().table_name()
        );
        let connection = self.jdbc_get_connection(request.identity()).await?;
        let result = async {
            let statement = PreparedStatementBuilder::new()
                .with_connection(connection.as_ref())
                .with_query(PostGreSqlMetadataHandler::GET_PARTITIONS_QUERY)?
                .with_parameters(vec![
                    request.table_name().schema_name().to_string(),
                    request.table_name().table_name().to_string(),
                ])?
                .build()
                .await?;
            let mut result_set = connection.execute_query(&statement).await?;
            if !result_set.next().await? {
                return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                    block.set_value(
                        PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                        row_num,
                        PostGreSqlMetadataHandler::ALL_PARTITIONS,
                    )?;
                    block.set_value(
                        PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
                        row_num,
                        PostGreSqlMetadataHandler::ALL_PARTITIONS,
                    )?;
                    Ok(1)
                });
            }
            loop {
                let partition_schema_name = result_set.get_string(PARTITION_SCHEMA_NAME)?;
                let partition_name = result_set.get_string(PARTITION_NAME)?;
                block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                    block.set_value(
                        PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                        row_num,
                        partition_schema_name.clone(),
                    )?;
                    block.set_value(
                        PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
                        row_num,
                        partition_name.clone(),
                    )?;
                    Ok(1)
                })?;
                if !result_set.next().await? {
                    return Ok(());
                }
            }
        }
        .await;
        let closed = connection.close().await;
        match (result, closed) {
            (Err(e), _) => Err(e),
            (Ok(()), Err(e)) => Err(e.into()),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        tracing::info!(
            "doGetSplits, QueryId:{}: Catalog {}, table {}",
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
                usize::try_from(parse_int(token)?).unwrap_or(0)
            }
            _ => 0,
        };
        let mut splits = Vec::new();
        let partitions = request.partitions();
        let mut splitter_used = false;
        if partitions.row_count() == 1 {
            let schema_text = partition_text(
                partitions,
                PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                0,
            )?;
            let partition = partition_text(
                partitions,
                PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
                0,
            )?;
            let (Some(schema_text), Some(partition)) = (schema_text, partition) else {
                return Err(FederationError::null_pointer(
                    "Cannot invoke \"org.apache.arrow.vector.util.Text.toString()\" because the return value of \"org.apache.arrow.vector.complex.reader.FieldReader.readText()\" is null",
                ));
            };
            let all = PostGreSqlMetadataHandler::ALL_PARTITIONS;
            if schema_text == all && partition == all {
                for split_clause in self
                    .get_split_clauses(request.table_name(), Some(request))
                    .await
                {
                    splits.push(
                        self.make_split(request, schema_text.clone(), split_clause)
                            .await?,
                    );
                    if splits.len() >= MAX_SPLITS_PER_REQUEST {
                        return Err(FederationError::Runtime(format!(
                            "Max splits supported with splitter {MAX_SPLITS_PER_REQUEST}"
                        )));
                    }
                    splitter_used = true;
                }
            }
        }
        if !splitter_used {
            for cur_partition in partition_contd..partitions.row_count() {
                let schema_text = partition_text(
                    partitions,
                    PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                    cur_partition,
                )?;
                let partition = partition_text(
                    partitions,
                    PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
                    cur_partition,
                )?;
                tracing::info!(
                    "{}: Input partition is {:?}",
                    request.query_id().unwrap_or("null"),
                    partition
                );
                splits.push(
                    self.make_split(request, java_string(schema_text), java_string(partition))
                        .await?,
                );
                if splits.len() >= MAX_SPLITS_PER_REQUEST {
                    return Ok(GetSplitsResponse::new(
                        request.catalog_name(),
                        splits,
                        Some((cur_partition + 1).to_string()),
                    ));
                }
            }
        }
        Ok(GetSplitsResponse::new(request.catalog_name(), splits, None))
    }

    async fn do_get_data_source_capabilities(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            capabilities(self.base.config_options())?,
        ))
    }
}

#[async_trait]
impl JdbcMetadataHandler for PostGreSqlMetadataHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    async fn list_paginated_tables(
        &self,
        connection: &dyn JdbcConnection,
        list_tables_request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        let token = list_tables_request.next_token();
        let page_size = list_tables_request.page_size();
        let t = match token {
            Some(token) => parse_int(token)?,
            None => 0,
        };
        tracing::info!(
            "Starting pagination at {:?} with page size {}",
            token,
            page_size
        );
        let paginated_tables = self
            .get_paginated_results(connection, list_tables_request.schema_name(), t, page_size)
            .await?;
        let next = t.wrapping_add(page_size);
        tracing::info!(
            "{} tables returned. Next token is {}",
            paginated_tables.len(),
            next
        );
        let short_page = usize::try_from(page_size).is_ok_and(|size| paginated_tables.len() < size)
            || page_size < 0;
        let next_token = if paginated_tables.is_empty() || short_page {
            None
        } else {
            Some(next.to_string())
        };
        Ok(ListTablesResponse::new(
            list_tables_request.catalog_name(),
            paginated_tables,
            next_token,
        ))
    }

    async fn list_tables(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        let mut list = JdbcUtil::get_tables(jdbc_connection, database_name).await?;
        list.extend(
            self.get_materialized_views(jdbc_connection, database_name)
                .await?,
        );
        Ok(list)
    }

    fn get_partition_schema(&self, _catalog_name: &str) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        schema_builder.add_field_with_type(
            PostGreSqlMetadataHandler::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
            DataType::Utf8,
        );
        schema_builder.add_field_with_type(
            PostGreSqlMetadataHandler::BLOCK_PARTITION_COLUMN_NAME,
            DataType::Utf8,
        );
        Ok(schema_builder.build())
    }

    fn get_array_arrow_type_from_type_name(
        &self,
        type_name: Option<&str>,
        precision: i32,
        scale: i32,
    ) -> ArrowType {
        match type_name {
            Some("_bool") => DataType::Boolean,
            Some("_int2") => DataType::Int16,
            Some("_int4") => DataType::Int32,
            Some("_int8") => DataType::Int64,
            Some("_float4") => DataType::Float32,
            Some("_float8") => DataType::Float64,
            Some("_date") => DataType::Date32,
            Some("_timestamp" | "_timestamptz") => DataType::Date64,
            Some("_numeric") => DataType::Decimal128(
                u8::try_from(precision).unwrap_or(u8::MAX),
                i8::try_from(scale).unwrap_or(i8::MAX),
            ),
            _ => DataType::Utf8,
        }
    }

    fn wrap_name_with_escaped_character(&self, input: &str) -> String {
        format!("\"{}\"", input.replace('"', "\"\""))
    }
}
