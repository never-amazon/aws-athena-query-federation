#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use athena_federation_sdk::data::block_utils;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use arrow::datatypes::{DataType, Schema};
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{BlockAllocator, BlockAllocatorImpl, SchemaBuilder};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::optimizations::pushdown::{
    ComplexExpressionPushdownSubType, FilterPushdownSubType, HintsSubtype,
};
use athena_federation_sdk::metadata::optimizations::DataSourceOptimizations;
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    ListTablesRequest,
};
use athena_federation_sdk::security::FederatedIdentity;
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::MetadataDialect;
use athena_jdbc::sql::{
    ColumnInfo, ColumnMetadata, JdbcType, PreparedStatement, SqlError, SqlValue,
};
use athena_postgresql::{
    char_columns, is_valid_split_column, split_clauses, wrap_name_with_escaped_character,
    PostGreSqlMetadataDialect, PostGreSqlMetadataHandler,
};
use indexmap::IndexMap;
use jdbc_support::{
    FakeConnection, FakeConnectionFactory, FakeSecretsManager, Fakes, GetPrimaryKeysCall,
    ResultSetData,
};

const CATALOG_NAME: &str = "testCatalog";
const TEST_SCHEMA: &str = "testSchema";
const TEST_TABLE: &str = "testTable";
const TEST_QUERY_ID: &str = "testQueryId";
const TEST_BUCKET: &str = "test-bucket";
const TEST_PREFIX: &str = "test-prefix";
const SCHEMA_COLUMN: &str = "schema_name";
const TABLE_NAME_COLUMN: &str = "TABLE_NAME";
const TABLE_SCHEMA_COLUMN: &str = "TABLE_SCHEM";
const COLUMN_NAME: &str = "column_name";
const CHILD_SCHEMA: &str = "child_schema";
const CHILD: &str = "child";
const BLOCK_PARTITION_SCHEMA_COLUMN_NAME: &str = "partition_schema_name";
const BLOCK_PARTITION_COLUMN_NAME: &str = "partition_name";
const MAX_SPLITS_PER_REQUEST: usize = 1000;
const QUERY_PASSTHROUGH: &str = "supports_query_passthrough";
const FILTER_PUSHDOWN_SIZE: usize = 2;
const COMPLEX_EXPRESSION_SIZE: usize = 1;
const HINTS_SIZE: usize = 1;
const TABLES_SQL: &str = "SELECT table_name as \"TABLE_NAME\", table_schema as \"TABLE_SCHEM\" FROM information_schema.tables WHERE table_schema = ?";
const MATERIALIZED_VIEWS_SQL: &str = "select matviewname as \"TABLE_NAME\", schemaname as \"TABLE_SCHEM\" from pg_catalog.pg_matviews mv where has_table_privilege(format('%I.%I', mv.schemaname, mv.matviewname), 'select') and schemaname = ?";
const SCHEMA_SQL: &str =
    "SELECT schema_name FROM information_schema.schemata WHERE lower(schema_name) = ?";
const TABLE_SQL: &str = "SELECT table_name FROM information_schema.tables WHERE table_schema = ? AND lower(table_name) = ?";
const MATVIEW_LOOKUP_SQL: &str = "select matviewname as \"table_name\" from pg_catalog.pg_matviews mv where schemaname = ? and lower(matviewname) = ?";
const DATA_TYPE_SQL: &str = "SELECT data_type FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND column_name = ?";
const CHAR_COLUMNS_SQL: &str = "SELECT column_name FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND data_type = 'character'";

struct Fixture {
    handler: PostGreSqlMetadataHandler,
    connection: FakeConnection,
    allocator: Arc<dyn BlockAllocator>,
}

fn config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new(
        CATALOG_NAME,
        "postgres",
        "postgres://jdbc:postgresql://hostname/user=A&password=B",
    )
    .unwrap()
}

fn fakes() -> Fakes {
    Fakes::with_secrets(FakeSecretsManager::with_secret(
        "testSecret",
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ))
}

fn handler_with(
    connection: &FakeConnection,
    config_options: HashMap<String, String>,
) -> PostGreSqlMetadataHandler {
    let factory = Arc::new(FakeConnectionFactory::new(connection.clone()));
    let config = config();
    let context = MetadataContext::builder(config.engine(), fakes().clients())
        .config_options(config_options)
        .encryption_key_factory(None)
        .spill_bucket(None)
        .spill_prefix(None)
        .build()
        .unwrap();
    PostGreSqlMetadataDialect::handler(context, config, factory)
}

fn setup() -> Fixture {
    let connection = FakeConnection::new();
    Fixture {
        handler: handler_with(&connection, HashMap::new()),
        connection,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn identity_with(config_options: HashMap<String, Option<String>>) -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), config_options)
}

fn identity() -> FederatedIdentity {
    identity_with(HashMap::new())
}

fn strings(labels: &[&str], rows: &[&[&str]]) -> ResultSetData {
    ResultSetData::of_strings(labels, rows)
}

fn tables(rows: &[(&str, &str)]) -> ResultSetData {
    let rows: Vec<[&str; 2]> = rows.iter().map(|(s, t)| [*s, *t]).collect();
    let refs: Vec<&[&str]> = rows.iter().map(<[&str; 2]>::as_slice).collect();
    strings(&[TABLE_SCHEMA_COLUMN, TABLE_NAME_COLUMN], &refs)
}

fn single_column(label: &str, values: &[&str]) -> ResultSetData {
    let rows: Vec<[&str; 1]> = values.iter().map(|v| [*v]).collect();
    let refs: Vec<&[&str]> = rows.iter().map(<[&str; 1]>::as_slice).collect();
    strings(&[label], &refs)
}

fn route(connection: &FakeConnection, routes: Vec<(&'static str, ResultSetData)>) {
    connection.on_query(move |statement: &PreparedStatement| {
        Ok(routes
            .iter()
            .find(|(sql, _)| statement.sql() == *sql)
            .map_or_else(|| ResultSetData::empty(&[]), |(_, data)| data.clone()))
    });
}

fn partition_schema(handler: &PostGreSqlMetadataHandler) -> Schema {
    handler.partition_schema(CATALOG_NAME).unwrap()
}

fn layout_request(
    handler: &PostGreSqlMetadataHandler,
    constraints: Constraints,
) -> GetTableLayoutRequest {
    let schema = partition_schema(handler);
    let cols: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    GetTableLayoutRequest::new(
        Some(identity()),
        Some(TEST_QUERY_ID.to_string()),
        CATALOG_NAME,
        TableName::new(TEST_SCHEMA, TEST_TABLE),
        constraints,
        schema,
        cols,
    )
}

fn partitions(rows: &[(&str, &str)]) -> ResultSetData {
    let rows: Vec<[&str; 2]> = rows.iter().map(|(s, t)| [*s, *t]).collect();
    let refs: Vec<&[&str]> = rows.iter().map(<[&str; 2]>::as_slice).collect();
    strings(&[CHILD_SCHEMA, CHILD], &refs)
}

async fn table_layout(
    f: &Fixture,
    handler: &PostGreSqlMetadataHandler,
    rows: &[(&str, &str)],
    constraints: Constraints,
) -> GetTableLayoutResponse {
    route(
        &f.connection,
        vec![(athena_postgresql::GET_PARTITIONS_QUERY, partitions(rows))],
    );
    handler
        .get_table_layout(f.allocator.as_ref(), &layout_request(handler, constraints))
        .await
        .unwrap()
}

fn partition_strings(response: &GetTableLayoutResponse) -> Vec<String> {
    let block = response.partitions();
    (0..block.row_count())
        .map(|i| block_utils::row_to_string(block, i).unwrap())
        .collect()
}

fn expected_partition_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_field_with_type(BLOCK_PARTITION_SCHEMA_COLUMN_NAME, DataType::Utf8);
    builder.add_field_with_type(BLOCK_PARTITION_COLUMN_NAME, DataType::Utf8);
    builder.build()
}

fn partition_cols(handler: &PostGreSqlMetadataHandler) -> Vec<String> {
    partition_schema(handler)
        .fields()
        .iter()
        .map(|f| f.name().clone())
        .collect()
}

fn splits_request(
    handler: &PostGreSqlMetadataHandler,
    identity: FederatedIdentity,
    layout: &GetTableLayoutResponse,
    constraints: Constraints,
    token: Option<&str>,
) -> GetSplitsRequest {
    GetSplitsRequest::new(
        Some(identity),
        Some(TEST_QUERY_ID.to_string()),
        "testCatalogName",
        TableName::new(TEST_SCHEMA, TEST_TABLE),
        layout.partitions().clone(),
        partition_cols(handler),
        constraints,
        token.map(ToString::to_string),
    )
}

fn split_properties(response: &GetSplitsResponse) -> Vec<BTreeMap<String, Option<String>>> {
    let mut props: Vec<BTreeMap<String, Option<String>>> = response
        .splits()
        .iter()
        .map(|s: &Split| s.properties().clone().into_iter().collect())
        .collect();
    props.sort();
    props
}

fn partition_map(schema: &str, name: &str) -> BTreeMap<String, Option<String>> {
    BTreeMap::from([
        (
            BLOCK_PARTITION_SCHEMA_COLUMN_NAME.to_string(),
            Some(schema.to_string()),
        ),
        (
            BLOCK_PARTITION_COLUMN_NAME.to_string(),
            Some(name.to_string()),
        ),
    ])
}

fn string_params(statement: &PreparedStatement) -> Vec<SqlValue> {
    statement.parameters().to_vec()
}

const ARRAY: JdbcType = JdbcType::Array;

type ColumnRow<'a> = (JdbcType, &'a str, Option<i32>, Option<i32>, Option<&'a str>);

fn columns(rows: &[ColumnRow<'_>]) -> Vec<ColumnInfo> {
    rows.iter()
        .map(|(data_type, name, size, digits, type_name)| ColumnInfo {
            name: Some((*name).to_string()),
            data_type: *data_type,
            type_name: type_name.map(ToString::to_string),
            column_size: *size,
            decimal_digits: *digits,
            ordinal_position: 0,
        })
        .collect()
}

fn get_table_request(table: TableName) -> GetTableRequest {
    GetTableRequest::new(
        Some(identity()),
        Some(TEST_QUERY_ID.to_string()),
        CATALOG_NAME,
        table,
        HashMap::new(),
    )
}

fn with_partition_fields(
    handler: &PostGreSqlMetadataHandler,
    builder: &mut SchemaBuilder,
) -> Schema {
    for field in partition_schema(handler).fields() {
        builder.add_field(Arc::clone(field));
    }
    builder.build()
}

fn capabilities_request() -> GetDataSourceCapabilitiesRequest {
    GetDataSourceCapabilitiesRequest::new(
        Some(identity()),
        Some(TEST_QUERY_ID.to_string()),
        CATALOG_NAME,
    )
}

fn verify_common_capabilities(response: &GetDataSourceCapabilitiesResponse) {
    assert_eq!(CATALOG_NAME, response.catalog_name());
    let capabilities = response.capabilities();
    let filter = capabilities
        .get(DataSourceOptimizations::SupportsFilterPushdown.optimization())
        .expect("filter pushdown capability");
    assert_eq!(FILTER_PUSHDOWN_SIZE, filter.len());
    assert!(filter
        .iter()
        .any(|s| s.sub_type() == FilterPushdownSubType::SortedRangeSet.sub_type()));
    assert!(filter
        .iter()
        .any(|s| s.sub_type() == FilterPushdownSubType::NullableComparison.sub_type()));
    let complex = capabilities
        .get(DataSourceOptimizations::SupportsComplexExpressionPushdown.optimization())
        .expect("complex expression capability");
    assert_eq!(COMPLEX_EXPRESSION_SIZE, complex.len());
    assert!(complex.iter().any(|s| s.sub_type()
        == ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes.sub_type()
        && !s.properties().is_empty()));
}

fn wrapped(column: &str) -> String {
    format!("\"{}\"", column.replace('"', "\"\""))
}

fn min_max_sql(column: &str) -> String {
    let column = wrapped(column);
    format!("select min({column}), max({column}) from \"testSchema\".\"testTable\"")
}

fn min_max(min: i32, max: i32) -> ResultSetData {
    ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("min", JdbcType::Integer),
            ColumnMetadata::new("max", JdbcType::Integer),
        ],
        vec![vec![SqlValue::Integer(min), SqlValue::Integer(max)]],
    )
}

fn split_clause_routes(
    connection: &FakeConnection,
    primary_key: Option<&str>,
    data_type: Option<&'static str>,
    bounds: Option<(i32, i32)>,
) {
    connection.meta.set_primary_keys(Ok(primary_key
        .map(|pk| Some(pk.to_string()))
        .into_iter()
        .collect()));
    let bounds_sql = primary_key.map(min_max_sql);
    connection.on_query(move |statement: &PreparedStatement| {
        let sql = statement.sql();
        if sql == DATA_TYPE_SQL {
            return Ok(match data_type {
                Some(t) => single_column("data_type", &[t]),
                None => ResultSetData::empty(&["data_type"]),
            });
        }
        if bounds_sql.as_deref() == Some(sql) {
            if let Some((min, max)) = bounds {
                return Ok(min_max(min, max));
            }
        }
        Ok(ResultSetData::empty(&[]))
    });
}

fn assert_primary_keys_lookup(connection: &FakeConnection, table: &str) {
    assert_eq!(
        connection.meta.get_primary_keys_calls(),
        [GetPrimaryKeysCall {
            catalog: None,
            schema: Some(TEST_SCHEMA.to_string()),
            table: table.to_string(),
        }]
    );
}

fn all_array_types_builder() -> SchemaBuilder {
    let mut builder = SchemaBuilder::new_builder();
    builder
        .add_list_field("bool_array", DataType::Boolean)
        .add_list_field("smallint_array", DataType::Int16)
        .add_list_field("int_array", DataType::Int32)
        .add_list_field("bigint_array", DataType::Int64)
        .add_list_field("float_array", DataType::Float32)
        .add_list_field("double_array", DataType::Float64)
        .add_list_field("date_array", DataType::Date32)
        .add_list_field("timestamp_array", DataType::Date64)
        .add_list_field("binary_array", DataType::Utf8)
        .add_list_field("decimal_array", DataType::Decimal128(38, 2))
        .add_list_field("string_array", DataType::Utf8)
        .add_list_field("uuid_array", DataType::Utf8);
    builder
}

#[tokio::test]
async fn do_list_paginated_tables() {
    let f = setup();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    f.connection.on_query(move |statement: &PreparedStatement| {
        if statement.sql() == athena_postgresql::LIST_PAGINATED_TABLES_QUERY {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let table = if n == 0 { TEST_TABLE } else { "testTable2" };
            return Ok(tables(&[(TEST_SCHEMA, table)]));
        }
        if statement.sql() == SCHEMA_SQL {
            return Ok(single_column(SCHEMA_COLUMN, &[TEST_SCHEMA]));
        }
        Ok(ResultSetData::empty(&[]))
    });
    let request = ListTablesRequest::new(
        Some(identity()),
        Some(TEST_QUERY_ID.to_string()),
        CATALOG_NAME,
        Some(TEST_SCHEMA.to_string()),
        None,
        1,
    );
    let response = f.handler.list_tables(&request).await.unwrap();
    assert_eq!(Some("1"), response.next_token());
    assert_eq!(
        &[TableName::new(TEST_SCHEMA, TEST_TABLE)],
        response.tables()
    );

    let request = ListTablesRequest::new(
        Some(identity()),
        Some(TEST_QUERY_ID.to_string()),
        CATALOG_NAME,
        Some(TEST_SCHEMA.to_string()),
        Some("1".to_string()),
        10,
    );
    let response = f.handler.list_tables(&request).await.unwrap();
    assert_eq!(
        &[TableName::new(TEST_SCHEMA, "testTable2")],
        response.tables()
    );
    assert_eq!(None, response.next_token());
}

#[tokio::test]
async fn get_partition_schema() {
    let f = setup();
    assert_eq!(expected_partition_schema(), partition_schema(&f.handler));
}

#[tokio::test]
async fn do_get_table_layout() {
    let f = setup();
    let response = table_layout(
        &f,
        &f.handler,
        &[("s0", "p0"), ("s1", "p1")],
        Constraints::empty(),
    )
    .await;
    let executed = f.connection.executed();
    let partitions_query = executed
        .iter()
        .find(|s| s.sql() == athena_postgresql::GET_PARTITIONS_QUERY)
        .unwrap();
    assert_eq!(
        vec![
            SqlValue::String(TEST_SCHEMA.to_string()),
            SqlValue::String(TEST_TABLE.to_string())
        ],
        string_params(partitions_query)
    );
    assert_eq!(2, response.partitions().row_count());
    assert_eq!(
        vec![
            "[partition_schema_name : s0], [partition_name : p0]",
            "[partition_schema_name : s1], [partition_name : p1]"
        ],
        partition_strings(&response)
    );
    assert_eq!(
        &expected_partition_schema(),
        response.partitions().schema().as_ref()
    );
    assert_eq!(
        &TableName::new(TEST_SCHEMA, TEST_TABLE),
        response.table_name()
    );
}

#[tokio::test]
async fn test_list_tables_with_valid_connection() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    connection.set_catalog(Some(CATALOG_NAME));
    route(
        &connection,
        vec![
            (TABLES_SQL, tables(&[(TEST_SCHEMA, "table1")])),
            (MATERIALIZED_VIEWS_SQL, tables(&[(TEST_SCHEMA, "matview1")])),
        ],
    );
    let result = f
        .handler
        .list_schema_tables(&connection, TEST_SCHEMA)
        .await
        .unwrap();
    assert_eq!(
        vec![
            TableName::new(TEST_SCHEMA, "table1"),
            TableName::new(TEST_SCHEMA, "matview1")
        ],
        result
    );
}

#[tokio::test]
async fn test_list_tables_with_no_materialized_views() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    route(
        &connection,
        vec![
            (TABLES_SQL, tables(&[(TEST_SCHEMA, "table1")])),
            (
                MATERIALIZED_VIEWS_SQL,
                ResultSetData::empty(&[TABLE_NAME_COLUMN]),
            ),
        ],
    );
    let result = f
        .handler
        .list_schema_tables(&connection, TEST_SCHEMA)
        .await
        .unwrap();
    assert_eq!(vec![TableName::new(TEST_SCHEMA, "table1")], result);
}

#[tokio::test]
async fn test_list_tables_with_database_connection_error() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.on_prepare(|_| Err(SqlError::new("Database error")));
    let error = f
        .handler
        .list_schema_tables(&connection, TEST_SCHEMA)
        .await
        .unwrap_err();
    let sql = SqlError::from_federation_error(&error).expect("SQLException");
    assert_eq!("Database error", sql.message());
}

#[tokio::test]
async fn do_get_table_layout_with_no_partitions() {
    let f = setup();
    let response = table_layout(&f, &f.handler, &[], Constraints::empty()).await;
    let executed = f.connection.executed();
    let partitions_query = executed
        .iter()
        .find(|s| s.sql() == athena_postgresql::GET_PARTITIONS_QUERY)
        .unwrap();
    assert_eq!(
        vec![
            SqlValue::String(TEST_SCHEMA.to_string()),
            SqlValue::String(TEST_TABLE.to_string())
        ],
        string_params(partitions_query)
    );
    assert_eq!(1, response.partitions().row_count());
    assert_eq!(
        vec!["[partition_schema_name : *], [partition_name : *]"],
        partition_strings(&response)
    );
    assert_eq!(
        &expected_partition_schema(),
        response.partitions().schema().as_ref()
    );
    assert_eq!(
        &TableName::new(TEST_SCHEMA, TEST_TABLE),
        response.table_name()
    );
}

#[tokio::test]
async fn do_get_table_layout_with_sql_exception() {
    let f = setup();
    f.connection
        .on_query(|_| Err(SqlError::new("partition query failed")));
    let result = f
        .handler
        .get_table_layout(
            f.allocator.as_ref(),
            &layout_request(&f.handler, Constraints::empty()),
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn do_get_splits() {
    let f = setup();
    let layout = table_layout(
        &f,
        &f.handler,
        &[("s0", "p0"), ("s1", "p1")],
        Constraints::empty(),
    )
    .await;
    let request = splits_request(&f.handler, identity(), &layout, Constraints::empty(), None);
    let response = f.handler.get_splits(&request).await.unwrap();
    let expected = vec![partition_map("s0", "p0"), partition_map("s1", "p1")];
    assert_eq!(expected.len(), response.splits().len());
    assert_eq!(expected, split_properties(&response));
}

#[tokio::test]
async fn do_get_splits_with_catalog_casing_filter() {
    let f = setup();
    let layout = table_layout(&f, &f.handler, &[("s0", "p0")], Constraints::empty()).await;
    let identity = identity_with(HashMap::from([(
        environment_constants::CATALOG_CASING_FILTER.to_string(),
        Some(environment_constants::UPPERCASE_ONLY.to_string()),
    )]));
    let request = splits_request(&f.handler, identity, &layout, Constraints::empty(), None);
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(1, response.splits().len());
    assert_eq!(
        Some(environment_constants::UPPERCASE_ONLY),
        response.splits()[0].get_property(environment_constants::CATALOG_CASING_FILTER)
    );
}

#[tokio::test]
async fn do_get_splits_continuation() {
    let f = setup();
    let layout = table_layout(
        &f,
        &f.handler,
        &[("s0", "p0"), ("s1", "p1")],
        Constraints::empty(),
    )
    .await;
    let request = splits_request(
        &f.handler,
        identity(),
        &layout,
        Constraints::empty(),
        Some("1"),
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    let expected = vec![partition_map("s1", "p1")];
    assert_eq!(expected.len(), response.splits().len());
    assert_eq!(expected, split_properties(&response));
}

#[tokio::test]
async fn do_get_table_with_array_columns() {
    let f = setup();
    f.connection.set_catalog(Some(CATALOG_NAME));
    f.connection.meta.set_columns(Ok(columns(&[
        (ARRAY, "bool_array", Some(0), Some(0), Some("_bool")),
        (ARRAY, "smallint_array", Some(0), Some(0), Some("_int2")),
        (ARRAY, "int_array", Some(0), Some(0), Some("_int4")),
        (ARRAY, "bigint_array", Some(0), Some(0), Some("_int8")),
        (ARRAY, "float_array", Some(0), Some(0), Some("_float4")),
        (ARRAY, "double_array", Some(0), Some(0), Some("_float8")),
        (ARRAY, "date_array", Some(0), Some(0), Some("_date")),
        (
            ARRAY,
            "timestamp_array",
            Some(0),
            Some(0),
            Some("_timestamp"),
        ),
        (ARRAY, "binary_array", Some(0), Some(0), Some("_bytea")),
        (ARRAY, "decimal_array", Some(38), Some(2), Some("_numeric")),
        (ARRAY, "string_array", Some(0), Some(0), Some("_text")),
        (ARRAY, "uuid_array", Some(0), Some(0), Some("_uuid")),
    ])));
    route(
        &f.connection,
        vec![
            (SCHEMA_SQL, single_column(SCHEMA_COLUMN, &[TEST_SCHEMA])),
            (TABLE_SQL, single_column("table_name", &[TEST_TABLE])),
        ],
    );
    let mut builder = all_array_types_builder();
    let expected = with_partition_fields(&f.handler, &mut builder);
    let response = f
        .handler
        .get_table(&get_table_request(TableName::new(TEST_SCHEMA, "testtable")))
        .await
        .unwrap();
    let calls = f.connection.meta.get_columns_calls();
    assert_eq!(Some(CATALOG_NAME.to_string()), calls[0].catalog);
    assert_eq!(Some(TEST_SCHEMA.to_string()), calls[0].schema_pattern);
    assert_eq!(Some(TEST_TABLE.to_string()), calls[0].table_name_pattern);
    assert_eq!(&expected, response.schema());
    assert_eq!(
        &TableName::new(TEST_SCHEMA, TEST_TABLE),
        response.table_name()
    );
    assert_eq!(CATALOG_NAME, response.catalog_name());
}

#[tokio::test]
async fn do_get_table_materialized_view() {
    let f = setup();
    f.connection.set_catalog(Some(CATALOG_NAME));
    f.connection.meta.set_columns(Ok(columns(&[
        (ARRAY, "bool_array", Some(0), Some(0), Some("_bool")),
        (ARRAY, "int_array", Some(0), Some(0), Some("_int4")),
        (ARRAY, "bigint_array", Some(0), Some(0), Some("_int8")),
        (ARRAY, "float_array", Some(0), Some(0), Some("_float4")),
    ])));
    route(
        &f.connection,
        vec![
            (SCHEMA_SQL, single_column(SCHEMA_COLUMN, &[TEST_SCHEMA])),
            (TABLE_SQL, ResultSetData::empty(&["table_name"])),
            (
                MATVIEW_LOOKUP_SQL,
                single_column("table_name", &["testMatView"]),
            ),
        ],
    );
    let mut builder = SchemaBuilder::new_builder();
    builder
        .add_list_field("bool_array", DataType::Boolean)
        .add_list_field("int_array", DataType::Int32)
        .add_list_field("bigint_array", DataType::Int64)
        .add_list_field("float_array", DataType::Float32);
    let expected = with_partition_fields(&f.handler, &mut builder);
    let response = f
        .handler
        .get_table(&get_table_request(TableName::new(
            TEST_SCHEMA,
            "testmatview",
        )))
        .await
        .unwrap();
    let calls = f.connection.meta.get_columns_calls();
    assert_eq!(Some("testMatView".to_string()), calls[0].table_name_pattern);
    assert_eq!(&expected, response.schema());
    assert_eq!(
        &TableName::new(TEST_SCHEMA, "testMatView"),
        response.table_name()
    );
    assert_eq!(CATALOG_NAME, response.catalog_name());
}

#[tokio::test]
async fn test_do_get_data_source_capabilities_without_query_passthrough() {
    let f = setup();
    let response = f
        .handler
        .get_data_source_capabilities(&capabilities_request())
        .await
        .unwrap();
    verify_common_capabilities(&response);
    assert!(response.capabilities().get(QUERY_PASSTHROUGH).is_none());
}

#[tokio::test]
async fn test_do_get_data_source_capabilities_with_query_passthrough() {
    let f = setup();
    let handler = handler_with(
        &f.connection,
        HashMap::from([("enable_query_passthrough".to_string(), "true".to_string())]),
    );
    let response = handler
        .get_data_source_capabilities(&capabilities_request())
        .await
        .unwrap();
    verify_common_capabilities(&response);
    let passthrough = response
        .capabilities()
        .get("SYSTEM.QUERY")
        .expect("Query passthrough should be present when enabled.");
    assert!(!passthrough.is_empty());
}

#[tokio::test]
async fn test_do_get_data_source_capabilities_with_non_default_collate() {
    let f = setup();
    let handler = handler_with(
        &f.connection,
        HashMap::from([("non_default_collate".to_string(), "true".to_string())]),
    );
    let response = handler
        .get_data_source_capabilities(&capabilities_request())
        .await
        .unwrap();
    verify_common_capabilities(&response);
    let hints = response
        .capabilities()
        .get(DataSourceOptimizations::DataSourceHints.optimization())
        .expect("data_source_hints capability");
    assert_eq!(HINTS_SIZE, hints.len());
    assert!(hints
        .iter()
        .any(|s| s.sub_type() == HintsSubtype::NonDefaultCollate.sub_type()));
}

#[tokio::test]
async fn test_schema_case_sensitivity_handling() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    route(
        &connection,
        vec![
            (SCHEMA_SQL, single_column(SCHEMA_COLUMN, &["TestSchema"])),
            (TABLES_SQL, tables(&[("TestSchema", "table1")])),
            (
                MATERIALIZED_VIEWS_SQL,
                ResultSetData::empty(&[TABLE_NAME_COLUMN]),
            ),
        ],
    );
    let result = f
        .handler
        .list_schema_tables(&connection, "testschema")
        .await
        .unwrap();
    assert_eq!(1, result.len());
    assert_eq!("TestSchema", result[0].schema_name());
    assert_eq!("table1", result[0].table_name());
}

#[tokio::test]
async fn test_table_name_case_sensitivity_handling() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    route(
        &connection,
        vec![
            (SCHEMA_SQL, single_column(SCHEMA_COLUMN, &[TEST_SCHEMA])),
            (TABLES_SQL, tables(&[(TEST_SCHEMA, "TestTable")])),
            (
                MATERIALIZED_VIEWS_SQL,
                ResultSetData::empty(&[TABLE_NAME_COLUMN]),
            ),
        ],
    );
    let result = f
        .handler
        .list_schema_tables(&connection, "testtable")
        .await
        .unwrap();
    assert_eq!(1, result.len());
    assert_eq!(TEST_SCHEMA, result[0].schema_name());
    assert_eq!("TestTable", result[0].table_name());
}

#[tokio::test]
async fn test_list_tables_with_invalid_schema() {
    let f = setup();
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    route(
        &connection,
        vec![
            (SCHEMA_SQL, ResultSetData::empty(&[SCHEMA_COLUMN])),
            (TABLES_SQL, ResultSetData::empty(&[TABLE_NAME_COLUMN])),
            (
                MATERIALIZED_VIEWS_SQL,
                ResultSetData::empty(&[TABLE_NAME_COLUMN]),
            ),
        ],
    );
    let result = f
        .handler
        .list_schema_tables(&connection, "nonexistentschema")
        .await
        .unwrap();
    assert!(result.is_empty(), "Expected empty list for invalid schema");
    let result = f.handler.list_schema_tables(&connection, "").await.unwrap();
    assert!(result.is_empty(), "Expected empty list for null schema");
}

#[tokio::test]
async fn test_get_char_columns_with_empty_result_set() {
    let connection = FakeConnection::new();
    route(
        &connection,
        vec![(CHAR_COLUMNS_SQL, ResultSetData::empty(&[COLUMN_NAME]))],
    );
    let char_columns = char_columns(&connection, TEST_SCHEMA, TEST_TABLE)
        .await
        .unwrap();
    let executed = connection.executed();
    assert_eq!(CHAR_COLUMNS_SQL, executed[0].sql());
    assert_eq!(
        vec![
            SqlValue::String(TEST_SCHEMA.to_string()),
            SqlValue::String(TEST_TABLE.to_string())
        ],
        string_params(&executed[0])
    );
    assert!(char_columns.is_empty());
}

#[tokio::test]
async fn test_get_char_columns_with_sql_exception() {
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlError::new("Database connection error")));
    let result = char_columns(&connection, TEST_SCHEMA, TEST_TABLE).await;
    assert_eq!("Database connection error", result.unwrap_err().message());
}

#[tokio::test]
async fn test_get_table_with_null_array_types() {
    let f = setup();
    f.connection.set_catalog(Some(CATALOG_NAME));
    f.connection.meta.set_columns(Ok(columns(&[
        (JdbcType::Array, "null_array", None, None, Some("_int4")),
        (ARRAY, "null_size_array", Some(0), None, Some("_numeric")),
        (JdbcType::Array, "null_type_array", Some(0), Some(0), None),
    ])));
    route(
        &f.connection,
        vec![
            (SCHEMA_SQL, single_column(SCHEMA_COLUMN, &[TEST_SCHEMA])),
            (TABLE_SQL, single_column("table_name", &[TEST_TABLE])),
        ],
    );
    let response = f
        .handler
        .get_table(&get_table_request(TableName::new(TEST_SCHEMA, TEST_TABLE)))
        .await
        .unwrap();
    let schema = response.schema();
    assert_eq!(5, schema.fields().len());
    for name in ["null_array", "null_size_array", "null_type_array"] {
        let field = schema.field_with_name(name).unwrap();
        assert!(matches!(field.data_type(), DataType::List(_)), "{name}");
    }
}

#[tokio::test]
async fn test_do_get_splits_with_partition_splitter() {
    let connection = FakeConnection::new();
    let config_options = HashMap::from([
        ("spill_bucket".to_string(), TEST_BUCKET.to_string()),
        ("spill_prefix".to_string(), TEST_PREFIX.to_string()),
        ("disable_spill_encryption".to_string(), "true".to_string()),
    ]);
    let handler = handler_with(&connection, config_options);
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        HashMap::new(),
        None,
    );
    connection
        .meta
        .set_primary_keys(Ok(vec![Some("id".to_string())]));
    let bounds_sql = min_max_sql("id");
    connection.on_query(move |statement: &PreparedStatement| {
        let sql = statement.sql();
        if sql == athena_postgresql::GET_PARTITIONS_QUERY {
            return Ok(partitions(&[("*", "*")]));
        }
        if sql == bounds_sql {
            return Ok(min_max(1, 100));
        }
        if sql == DATA_TYPE_SQL {
            return Ok(ResultSetData::empty(&["data_type"]));
        }
        Err(SqlError::new(format!("unexpected query {sql}")))
    });
    let layout = handler
        .get_table_layout(
            allocator.as_ref(),
            &layout_request(&handler, constraints.clone()),
        )
        .await
        .unwrap();
    let request = splits_request(&handler, identity(), &layout, constraints, None);
    let response = handler.get_splits(&request).await.unwrap();
    let expected_clauses = [
        "(\"id\" >= 1 AND \"id\" <= 5)",
        "(\"id\" >= 6 AND \"id\" <= 10)",
        "(\"id\" >= 11 AND \"id\" <= 15)",
        "(\"id\" >= 16 AND \"id\" <= 20)",
        "(\"id\" >= 21 AND \"id\" <= 25)",
        "(\"id\" >= 26 AND \"id\" <= 30)",
        "(\"id\" >= 31 AND \"id\" <= 35)",
        "(\"id\" >= 36 AND \"id\" <= 40)",
        "(\"id\" >= 41 AND \"id\" <= 45)",
        "(\"id\" >= 46 AND \"id\" <= 50)",
        "(\"id\" >= 51 AND \"id\" <= 55)",
        "(\"id\" >= 56 AND \"id\" <= 60)",
        "(\"id\" >= 61 AND \"id\" <= 65)",
        "(\"id\" >= 66 AND \"id\" <= 70)",
        "(\"id\" >= 71 AND \"id\" <= 75)",
        "(\"id\" >= 76 AND \"id\" <= 80)",
        "(\"id\" >= 81 AND \"id\" <= 85)",
        "(\"id\" >= 86 AND \"id\" <= 90)",
        "(\"id\" >= 91 AND \"id\" <= 95)",
        "(\"id\" >= 96 AND \"id\" <= 100)",
    ];
    assert_eq!(20, response.splits().len(), "Expected 20 splits");
    let mut names: Vec<String> = Vec::new();
    for split in response.splits() {
        assert_eq!(
            Some("*"),
            split.get_property(BLOCK_PARTITION_SCHEMA_COLUMN_NAME)
        );
        names.push(
            split
                .get_property(BLOCK_PARTITION_COLUMN_NAME)
                .unwrap()
                .to_string(),
        );
        assert!(
            !split.properties().is_empty(),
            "Split should have properties"
        );
    }
    names.sort_by_key(|name| expected_clauses.iter().position(|c| c == name));
    assert_eq!(expected_clauses.as_slice(), names.as_slice());
    assert!(
        response.splits().len() <= MAX_SPLITS_PER_REQUEST,
        "Number of splits should not exceed max splits"
    );
}

#[tokio::test]
async fn test_get_char_columns_with_character_columns() {
    let connection = FakeConnection::new();
    connection.meta.set_search_string_escape(Some("\\"));
    route(
        &connection,
        vec![(
            CHAR_COLUMNS_SQL,
            single_column(COLUMN_NAME, &["col1", "col2"]),
        )],
    );
    let char_columns = char_columns(&connection, TEST_SCHEMA, TEST_TABLE)
        .await
        .unwrap();
    let executed = connection.executed();
    assert_eq!(
        vec![
            SqlValue::String(TEST_SCHEMA.to_string()),
            SqlValue::String(TEST_TABLE.to_string())
        ],
        string_params(&executed[0])
    );
    assert_eq!(vec!["col1".to_string(), "col2".to_string()], char_columns);
}

#[tokio::test]
async fn test_get_array_arrow_type_from_type_name_maps_timestamptz_like_scalar_timestamptz() {
    let f = setup();
    let h = &f.handler;
    assert_eq!(
        athena_jdbc::manager::jdbc_to_arrow_type(JdbcType::Timestamp, 0, 0, &HashMap::new())
            .unwrap()
            .unwrap(),
        h.dialect().array_element_type(Some("_timestamptz"), 0, 0)
    );
    assert_eq!(
        DataType::Date64,
        h.dialect().array_element_type(Some("_timestamptz"), 0, 0)
    );
}

#[tokio::test]
async fn test_get_array_arrow_type_from_type_name() {
    let f = setup();
    let h = &f.handler;
    assert!(matches!(
        h.dialect().array_element_type(Some("_bool"), 0, 0),
        DataType::Boolean
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_int2"), 0, 0),
        DataType::Int16
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_int4"), 0, 0),
        DataType::Int32
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_int8"), 0, 0),
        DataType::Int64
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_float4"), 0, 0),
        DataType::Float32
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_float8"), 0, 0),
        DataType::Float64
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_date"), 0, 0),
        DataType::Date32
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_timestamp"), 0, 0),
        DataType::Date64
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_numeric"), 10, 2),
        DataType::Decimal128(10, 2)
    ));
    assert!(matches!(
        h.dialect().array_element_type(Some("_unknown"), 0, 0),
        DataType::Utf8
    ));
}

#[tokio::test]
async fn test_get_partition_schema() {
    let f = setup();
    let schema = partition_schema(&f.handler);
    assert_eq!(2, schema.fields().len());
    assert_eq!(
        athena_postgresql::BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
        schema.field(0).name()
    );
    assert_eq!(
        athena_postgresql::BLOCK_PARTITION_COLUMN_NAME,
        schema.field(1).name()
    );
}

#[tokio::test]
async fn get_split_clauses_no_primary_key_returns_empty_list() {
    let f = setup();
    split_clause_routes(&f.connection, None, None, None);
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    assert!(result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
}

#[tokio::test]
async fn get_split_clauses_uuid_primary_key_returns_empty_list() {
    let f = setup();
    split_clause_routes(&f.connection, Some("id"), Some("uuid"), None);
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    assert!(result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
    assert_eq!(f.connection.prepared_sql(), [DATA_TYPE_SQL]);
}

#[tokio::test]
async fn get_split_clauses_integer_primary_key_returns_split_clauses() {
    let f = setup();
    split_clause_routes(
        &f.connection,
        Some("order_id"),
        Some("integer"),
        Some((1, 100)),
    );
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    assert!(!result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
}

#[tokio::test]
async fn get_split_clauses_exception_during_processing_returns_empty_list() {
    let f = setup();
    f.connection
        .meta
        .set_primary_keys(Err(SqlError::new("Database connection error")));
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, "errorTable"), None).await;
    assert!(result.is_empty());
    assert_primary_keys_lookup(&f.connection, "errorTable");
}

#[tokio::test]
async fn get_split_clauses_uuid_check_returns_false_generates_splits() {
    let f = setup();
    split_clause_routes(&f.connection, Some("product_id"), None, Some((1, 50)));
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    assert!(!result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
}

#[tokio::test]
async fn get_split_clauses_uuid_check_throws_exception_generates_splits() {
    let f = setup();
    split_clause_routes(&f.connection, Some("id"), None, Some((1, 100)));
    f.connection.on_prepare(|sql| {
        if sql == DATA_TYPE_SQL {
            Err(SqlError::new("UUID check failed"))
        } else {
            Ok(PreparedStatement::new(sql))
        }
    });
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    assert!(!result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
}

#[tokio::test]
async fn get_split_clauses_malicious_primary_key_column_name_is_quoted_and_escaped_in_both_sinks() {
    let f = setup();
    let malicious_column = "id\");GRANT rds_superuser TO report_user;--";
    let wrapped_column = format!("\"{}\"", malicious_column.replace('"', "\"\""));
    split_clause_routes(&f.connection, Some(malicious_column), None, Some((1, 100)));
    let result = split_clauses(&f.handler, &TableName::new(TEST_SCHEMA, TEST_TABLE), None).await;
    let prepared = f.connection.prepared_sql();
    let bounds_sql = prepared
        .iter()
        .find(|sql| sql.contains("min("))
        .unwrap_or_else(|| panic!("bounds query was never prepared; calls: {prepared:?}"));
    assert!(
        bounds_sql.contains(&format!("min({wrapped_column})")),
        "{bounds_sql}"
    );
    assert!(!bounds_sql.contains("min(id\");"), "{bounds_sql}");
    assert!(!result.is_empty());
    assert_primary_keys_lookup(&f.connection, TEST_TABLE);
    for clause in &result {
        assert!(clause.contains(&wrapped_column), "{clause}");
        assert!(!clause.contains("id\") "), "{clause}");
    }
}

#[tokio::test]
async fn is_valid_split_column_rejects_null_empty_and_control_chars_accepts_normal() {
    assert!(is_valid_split_column("order_id"));
    assert!(is_valid_split_column("my order id"));
    assert!(!is_valid_split_column(""));
    assert!(!is_valid_split_column("id\nGRANT"));
    assert!(!is_valid_split_column("id\u{0}GRANT"));
}

#[tokio::test]
async fn wrap_name_with_escaped_character_escapes_embedded_double_quotes() {
    assert_eq!("\"order_id\"", wrap_name_with_escaped_character("order_id"));
    assert_eq!("\"a\"\"b\"", wrap_name_with_escaped_character("a\"b"));
    assert_eq!("\"x\"\");--\"", wrap_name_with_escaped_character("x\");--"));
}
