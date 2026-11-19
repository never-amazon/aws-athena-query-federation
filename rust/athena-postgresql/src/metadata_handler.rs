use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{ArrowType, Block, SchemaBuilder, SimpleBlockWriter};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{AwsClients, MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::optimizations::pushdown::HintsSubtype;
use athena_federation_sdk::metadata::optimizations::{Capability, OptimizationSubType};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::close_after;
use athena_jdbc::connection::{
    DatabaseConnectionConfig, DatabaseConnectionInfo, GenericJdbcConnectionFactory,
    JdbcConnectionFactory, JdbcProperties,
};
use athena_jdbc::manager::{
    complex_expression_capability, filter_capability, jdbc_util, ConnectionSource,
    JdbcMetadataHandler, MetadataDialect, PreparedStatementBuilder, TABLES_AND_VIEWS,
};
use athena_jdbc::splits::splitter;
use athena_jdbc::sql::{JdbcConnection, SqlError, SqlValue};
use indexmap::IndexMap;

use crate::constants;
use crate::driver::PgDriver;
use crate::resolver::postgresql_case_resolver;

const MATERIALIZED_VIEWS: &str = "Materialized Views";
const PARTITION_SCHEMA_NAME: &str = "child_schema";
const PARTITION_NAME: &str = "child";
const MAX_SPLITS_PER_REQUEST: usize = 1_000_000;
const GET_DATA_TYPE_QUERY: &str = "SELECT data_type FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND column_name = ?";
const MATERIALIZED_VIEWS_QUERY: &str = "select matviewname as \"TABLE_NAME\", schemaname as \"TABLE_SCHEM\" from pg_catalog.pg_matviews mv where has_table_privilege(format('%I.%I', mv.schemaname, mv.matviewname), 'select') and schemaname = ?";
const CHAR_COLUMNS_QUERY: &str = "SELECT column_name FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND data_type = 'character'";
const NUL_IDENTIFIER_MESSAGE: &str = "Zero bytes may not occur in identifiers.";

pub const JDBC_PROPERTIES: [(&str, &str); 1] = [("databaseTerm", "SCHEMA")];
pub const GET_PARTITIONS_QUERY: &str = "SELECT nmsp_child.nspname AS child_schema, child.relname AS child FROM pg_inherits JOIN pg_class parent ON pg_inherits.inhparent = parent.oid JOIN pg_class child ON pg_inherits.inhrelid = child.oid JOIN pg_namespace nmsp_parent ON nmsp_parent.oid = parent.relnamespace JOIN pg_namespace nmsp_child ON nmsp_child.oid = child.relnamespace where nmsp_parent.nspname = ? AND parent.relname = ?";
pub const BLOCK_PARTITION_COLUMN_NAME: &str = "partition_name";
pub const BLOCK_PARTITION_SCHEMA_COLUMN_NAME: &str = "partition_schema_name";
pub const ALL_PARTITIONS: &str = "*";
pub const LIST_PAGINATED_TABLES_QUERY: &str = "SELECT a.\"TABLE_NAME\", a.\"TABLE_SCHEM\" FROM ((SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ?) UNION (SELECT matviewname as \"TABLE_NAME\", schemaname as \"TABLE_SCHEM\" from pg_catalog.pg_matviews mv where has_table_privilege(format('%I.%I', mv.schemaname, mv.matviewname), 'select') and schemaname = ?)) AS a ORDER BY a.\"TABLE_NAME\" LIMIT ? OFFSET ?";
pub const NON_DEFAULT_COLLATE: &str = "non_default_collate";
pub const SQL_SPLITS_STRING: &str = "select min(%s), max(%s) from %s.%s";
pub const DEFAULT_NUM_SPLITS: i32 = 20;

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

pub struct PostGreSqlMetadataDialect;

pub type PostGreSqlMetadataHandler = JdbcMetadataHandler<PostGreSqlMetadataDialect>;

pub fn jdbc_properties() -> JdbcProperties {
    JDBC_PROPERTIES
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

impl PostGreSqlMetadataDialect {
    pub fn connection_factory(
        config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<GenericJdbcConnectionFactory> {
        GenericJdbcConnectionFactory::new_with_config_options(
            config.clone(),
            Some(jdbc_properties()),
            DatabaseConnectionInfo::new(
                constants::POSTGRESQL_DRIVER_CLASS,
                constants::POSTGRESQL_DEFAULT_PORT,
            )?,
            Some(config_options),
            Arc::new(PgDriver),
        )
    }
}

impl PostGreSqlMetadataDialect {
    pub fn handler(
        context: MetadataContext,
        config: DatabaseConnectionConfig,
        connection_factory: Arc<dyn JdbcConnectionFactory>,
    ) -> PostGreSqlMetadataHandler {
        JdbcMetadataHandler::new(
            context,
            ConnectionSource::Direct {
                config,
                factory: connection_factory,
            },
            postgresql_case_resolver(constants::POSTGRES_NAME),
            PostGreSqlMetadataDialect,
        )
    }

    pub fn from_config(
        clients: AwsClients,
        config: DatabaseConnectionConfig,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlMetadataHandler> {
        let factory = Arc::new(PostGreSqlMetadataDialect::connection_factory(
            &config,
            &config_options,
        )?);
        let context = MetadataContext::builder(config.engine(), clients)
            .config_options(config_options)
            .build()?;
        Ok(PostGreSqlMetadataDialect::handler(context, config, factory))
    }

    pub fn from_config_options(
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<PostGreSqlMetadataHandler> {
        let config =
            jdbc_util::single_database_config_from_env(constants::POSTGRES_NAME, &config_options)?;
        PostGreSqlMetadataDialect::from_config(clients, config, config_options)
    }
}

pub async fn get_char_columns(
    connection: &dyn JdbcConnection,
    schema: &str,
    table: &str,
) -> std::result::Result<Vec<String>, SqlError> {
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
        return Err(FederationError::runtime(format!(
            "Unable to escape SQL identifier: {NUL_IDENTIFIER_MESSAGE}"
        )));
    }
    Ok(format!("\"{}\"", input.replace('"', "\"\"")))
}

pub fn is_valid_split_column(column_name: Option<&str>) -> bool {
    column_name.is_some_and(|name| !name.is_empty() && !name.chars().any(char::is_control))
}

pub fn wrap_name_with_escaped_character(input: &str) -> String {
    format!("\"{}\"", input.replace('"', "\"\""))
}

async fn get_materialized_views(
    connection: &dyn JdbcConnection,
    database_name: &str,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let mut statement = connection
        .prepare_statement(MATERIALIZED_VIEWS_QUERY)
        .await?;
    statement.set_string(1, database_name)?;
    tracing::debug!(
        "Prepared Statement for getting Materialized View in schema {} : {}",
        database_name,
        statement.sql()
    );
    jdbc_util::table_metadata(connection, &statement, MATERIALIZED_VIEWS).await
}

async fn get_paginated_results(
    connection: &dyn JdbcConnection,
    database_name: Option<&str>,
    token: i32,
    limit: i32,
) -> std::result::Result<Vec<TableName>, SqlError> {
    let mut statement = connection
        .prepare_statement(LIST_PAGINATED_TABLES_QUERY)
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
    jdbc_util::table_metadata(connection, &statement, TABLES_AND_VIEWS).await
}

async fn is_uuid_column(
    connection: &dyn JdbcConnection,
    table_name: &TableName,
    column_name: &str,
) -> bool {
    let result: std::result::Result<bool, SqlError> = async {
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
    connection: &dyn JdbcConnection,
    table_name: &TableName,
) -> Result<Vec<String>> {
    let mut split_clauses = Vec::new();
    let primary_key_column = connection
        .metadata()
        .primary_key_columns(
            None,
            Some(table_name.schema_name()),
            table_name.table_name(),
        )
        .await?
        .into_iter()
        .next()
        .flatten();
    let Some(primary_key_column) = primary_key_column else {
        return Ok(split_clauses);
    };
    if !is_valid_split_column(Some(&primary_key_column)) {
        tracing::warn!(
            "Primary key column name is not a valid split identifier for table {}.{}. Skipping split generation.",
            table_name.schema_name(),
            table_name.table_name()
        );
        return Ok(split_clauses);
    }
    if is_uuid_column(connection, table_name, &primary_key_column).await {
        tracing::info!(
            "Primary key '{}' is UUID type. Skipping split generation for table: {}.{} (UUID columns do not support MIN/MAX functions in PostgreSQL)",
            primary_key_column,
            table_name.schema_name(),
            table_name.table_name()
        );
        return Ok(split_clauses);
    }
    let quoted_primary_key_column = escape_identifier(&primary_key_column)?;
    let sql = format!(
        "select min({quoted_primary_key_column}), max({quoted_primary_key_column}) from {}.{}",
        escape_identifier(table_name.schema_name())?,
        escape_identifier(table_name.table_name())?
    );
    let statement = connection.prepare_statement(&sql).await?;
    let mut min_max = connection.execute_query(&statement).await?;
    min_max.next().await?;
    let min = min_max.get_long(1usize)?.unwrap_or(0);
    let max = min_max.get_long(2usize)?.unwrap_or(0);
    let splitter = splitter(&quoted_primary_key_column, &min_max, DEFAULT_NUM_SPLITS)?;
    if let Some(mut splitter) = splitter {
        if max - min < i64::from(DEFAULT_NUM_SPLITS) {
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
    handler: &PostGreSqlMetadataHandler,
    table_name: &TableName,
    request: Option<&GetSplitsRequest>,
) -> Vec<String> {
    let result = async {
        let connection = handler
            .connect(request.and_then(GetSplitsRequest::identity))
            .await?;
        let clauses = split_clauses_from(connection.as_ref(), table_name).await;
        close_after(connection.as_ref(), clauses).await
    }
    .await;
    result.unwrap_or_else(|ex| {
        tracing::warn!("Unable to split data. {}", ex);
        Vec::new()
    })
}

async fn make_split(
    handler: &PostGreSqlMetadataHandler,
    request: &GetSplitsRequest,
    partition_schema: String,
    partition: String,
) -> Result<Split> {
    let mut builder = handler
        .context()
        .split_builder(request)
        .await?
        .add(BLOCK_PARTITION_SCHEMA_COLUMN_NAME, partition_schema)
        .add(BLOCK_PARTITION_COLUMN_NAME, partition);
    if let Some(filter) = catalog_casing_filter(request.identity()) {
        builder = builder.add(environment_constants::CATALOG_CASING_FILTER, filter);
    }
    Ok(builder.build())
}

fn capabilities(
    config_options: &HashMap<String, String>,
) -> Result<IndexMap<String, Vec<OptimizationSubType>>> {
    let mut capabilities = IndexMap::new();
    let (name, sub_types) = filter_capability().into_entry();
    capabilities.insert(name, sub_types);
    let (name, sub_types) = complex_expression_capability()?.into_entry();
    capabilities.insert(name, sub_types);
    athena_jdbc::qpt::JDBC_QUERY_PASSTHROUGH
        .add_query_passthrough_capability_if_enabled(&mut capabilities, config_options);
    let non_default_collate = config_options
        .get(NON_DEFAULT_COLLATE)
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if non_default_collate {
        let (name, sub_types) =
            Capability::Hints(vec![HintsSubtype::NonDefaultCollate]).into_entry();
        capabilities.insert(name, sub_types);
    }
    Ok(capabilities)
}

#[async_trait]
impl MetadataDialect for PostGreSqlMetadataDialect {
    fn partition_schema(
        &self,
        _handler: &PostGreSqlMetadataHandler,
        _catalog_name: &str,
    ) -> Result<Schema> {
        let mut schema_builder = SchemaBuilder::new_builder();
        schema_builder.add_field_with_type(BLOCK_PARTITION_SCHEMA_COLUMN_NAME, DataType::Utf8);
        schema_builder.add_field_with_type(BLOCK_PARTITION_COLUMN_NAME, DataType::Utf8);
        Ok(schema_builder.build())
    }

    async fn get_partitions(
        &self,
        handler: &PostGreSqlMetadataHandler,
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
        let connection = handler.connect(request.identity()).await?;
        let result = async {
            let statement = PreparedStatementBuilder::new()
                .with_connection(connection.as_ref())
                .with_query(GET_PARTITIONS_QUERY)?
                .with_parameters(vec![
                    request.table_name().schema_name().to_string(),
                    request.table_name().table_name().to_string(),
                ])?
                .build()
                .await?;
            let mut result_set = connection.execute_query(&statement).await?;
            if !result_set.next().await? {
                return block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                    block.set_value(BLOCK_PARTITION_SCHEMA_COLUMN_NAME, row_num, ALL_PARTITIONS)?;
                    block.set_value(BLOCK_PARTITION_COLUMN_NAME, row_num, ALL_PARTITIONS)?;
                    Ok(1)
                });
            }
            loop {
                let partition_schema_name = result_set.get_string(PARTITION_SCHEMA_NAME)?;
                let partition_name = result_set.get_string(PARTITION_NAME)?;
                block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
                    block.set_value(
                        BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                        row_num,
                        partition_schema_name.clone(),
                    )?;
                    block.set_value(
                        BLOCK_PARTITION_COLUMN_NAME,
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
        close_after(connection.as_ref(), result).await
    }

    async fn get_splits(
        &self,
        handler: &PostGreSqlMetadataHandler,
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
            return handler.query_passthrough_split(request).await;
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
            let schema_text = partition_text(partitions, BLOCK_PARTITION_SCHEMA_COLUMN_NAME, 0)?;
            let partition = partition_text(partitions, BLOCK_PARTITION_COLUMN_NAME, 0)?;
            let (Some(schema_text), Some(partition)) = (schema_text, partition) else {
                return Err(FederationError::null_pointer(
                    "Cannot invoke \"org.apache.arrow.vector.util.Text.toString()\" because the return value of \"org.apache.arrow.vector.complex.reader.FieldReader.readText()\" is null",
                ));
            };
            let all = ALL_PARTITIONS;
            if schema_text == all && partition == all {
                for split_clause in
                    get_split_clauses(handler, request.table_name(), Some(request)).await
                {
                    splits.push(
                        make_split(handler, request, schema_text.clone(), split_clause).await?,
                    );
                    if splits.len() >= MAX_SPLITS_PER_REQUEST {
                        return Err(FederationError::runtime(format!(
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
                    BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
                    cur_partition,
                )?;
                let partition =
                    partition_text(partitions, BLOCK_PARTITION_COLUMN_NAME, cur_partition)?;
                tracing::info!(
                    "{}: Input partition is {:?}",
                    request.query_id().unwrap_or("null"),
                    partition
                );
                splits.push(
                    make_split(
                        handler,
                        request,
                        java_string(schema_text),
                        java_string(partition),
                    )
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

    async fn list_paginated_tables(
        &self,
        _handler: &PostGreSqlMetadataHandler,
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
        let paginated_tables =
            get_paginated_results(connection, list_tables_request.schema_name(), t, page_size)
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

    async fn list_schema_tables(
        &self,
        _handler: &PostGreSqlMetadataHandler,
        connection: &dyn JdbcConnection,
        database_name: &str,
    ) -> Result<Vec<TableName>> {
        let mut list = jdbc_util::tables(connection, database_name).await?;
        list.extend(get_materialized_views(connection, database_name).await?);
        Ok(list)
    }

    fn array_element_type(&self, type_name: Option<&str>, precision: i32, scale: i32) -> ArrowType {
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

    fn capabilities(
        &self,
        handler: &PostGreSqlMetadataHandler,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            capabilities(handler.config_options())?,
        ))
    }
}
