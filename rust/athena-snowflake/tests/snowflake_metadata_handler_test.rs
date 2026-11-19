mod snowflake_support;

use athena_federation_sdk::resolver::FederationSdkCasingMode;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Field, Schema};
use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, FieldBuilder, SchemaBuilder, Value,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::handlers::{MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetSplitsRequest, GetTableLayoutRequest, GetTableRequest,
    ListSchemasRequest, ListTablesRequest,
};
use athena_federation_sdk::request::FederatedIdentity;
use athena_federation_sdk::resolver::CASING_MODE_CONFIGURATION_KEY;
use athena_federation_sdk::security::AwsRequestOverrideConfiguration;
use athena_federation_sdk::serde::{schema_from_bytes, schema_to_bytes, IpcFormat};
use athena_federation_sdk::FederationError;
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{ConnectionSource, MetadataDialect};
use athena_jdbc::resolver::JdbcCaseResolver;
use athena_jdbc::sql::{
    ColumnInfo, ColumnMetadata, JdbcType, PreparedStatement, SchemaEntry, SqlError, SqlValue,
};
use athena_snowflake::resolver::snowflake_case_resolver;
use athena_snowflake::{
    create_credentials_provider, create_snowflake_credentials_provider, snowflake_constants,
    SnowflakeMetadataDialect, SnowflakeMetadataHandler,
};
use indexmap::IndexMap;
use snowflake_support::{
    FakeConnection, FakeConnectionFactory, FakeS3Lister, FakeSecretsManager, Fakes, ResultSetData,
};

const CATALOG: &str = "testCatalog";
const PARTITION: &str = "partition";
const S3_COLUMN: &str = "s3_column_name_list";
const DESC_TEST_INTEGRATION: &str = "DESC STORAGE INTEGRATION \"TEST_INTEGRATION\"";

fn config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new(
        CATALOG,
        snowflake_constants::SNOWFLAKE_NAME,
        "snowflake://jdbc:snowflake://hostname/?warehouse=warehousename&db=dbname&schema=schemaname&user=xxx&password=xxx",
    )
    .unwrap()
}

fn fakes() -> Fakes {
    Fakes::with_secrets(FakeSecretsManager::with_secret(
        "testSecret",
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ))
}

fn base_options() -> HashMap<String, String> {
    HashMap::from([
        ("spill_bucket".to_string(), "test-bucket".to_string()),
        ("disable_spill_encryption".to_string(), "true".to_string()),
    ])
}

fn options_with(extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut options = base_options();
    for (key, value) in extra {
        options.insert((*key).to_string(), (*value).to_string());
    }
    options
}

fn export_options() -> HashMap<String, String> {
    options_with(&[
        (snowflake_constants::SNOWFLAKE_ENABLE_S3_EXPORT, "true"),
        (
            snowflake_constants::STORAGE_INTEGRATION_CONFIG_KEY,
            "TEST_INTEGRATION",
        ),
    ])
}

struct Fixture {
    handler: SnowflakeMetadataHandler,
    connection: FakeConnection,
    lister: FakeS3Lister,
    allocator: Arc<dyn BlockAllocator>,
}

fn fixture_with(lister: FakeS3Lister, config_options: HashMap<String, String>) -> Fixture {
    let connection = FakeConnection::new();
    let factory = Arc::new(FakeConnectionFactory::new(connection.clone()));
    let config = config();
    let context = MetadataContext::builder(config.engine(), fakes().clients())
        .config_options(config_options)
        .build()
        .unwrap();
    let case_resolver = JdbcCaseResolver::new(
        config.engine(),
        FederationSdkCasingMode::None,
        FederationSdkCasingMode::None,
    );
    let handler = SnowflakeMetadataDialect::handler(
        context,
        config,
        factory,
        case_resolver,
        Some(Arc::new(lister.clone())),
    );
    Fixture {
        handler,
        connection,
        lister,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn fixture() -> Fixture {
    fixture_with(FakeS3Lister::default(), base_options())
}

fn casing_fixture(mode: &str) -> Fixture {
    let connection = FakeConnection::new();
    let factory = Arc::new(FakeConnectionFactory::new(connection.clone()));
    let config = config();
    let context = MetadataContext::builder(config.engine(), fakes().clients())
        .config_options(options_with(&[(CASING_MODE_CONFIGURATION_KEY, mode)]))
        .build()
        .unwrap();
    let handler = SnowflakeMetadataDialect::handler(
        context,
        config,
        factory,
        snowflake_case_resolver(
            snowflake_constants::SNOWFLAKE_NAME,
            FederationSdkCasingMode::None,
        ),
        None,
    );
    Fixture {
        handler,
        connection,
        lister: FakeS3Lister::default(),
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn identity_with(config_options: HashMap<String, Option<String>>) -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), config_options)
}

fn identity() -> FederatedIdentity {
    identity_with(HashMap::new())
}

fn constraints() -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        HashMap::new(),
        None,
    )
}

fn qpt_constraints(query: &str) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        HashMap::from([
            (
                "schemaFunctionName".to_string(),
                Some("SYSTEM.QUERY".to_string()),
            ),
            ("QUERY".to_string(), Some(query.to_string())),
        ]),
        None,
    )
}

fn table_request(table: TableName) -> GetTableRequest {
    GetTableRequest::new(
        Some(identity()),
        Some("testQueryId".to_string()),
        CATALOG,
        table,
        HashMap::new(),
    )
}

fn column_row(data_type: JdbcType, size: i32, name: &str) -> ColumnInfo {
    ColumnInfo {
        name: Some(name.to_string()),
        data_type,
        type_name: None,
        column_size: Some(size),
        decimal_digits: Some(0),
        ordinal_position: 0,
    }
}

fn columns(rows: Vec<ColumnInfo>) -> Vec<ColumnInfo> {
    rows
}

fn schemas(rows: &[(&str, &str)]) -> Vec<SchemaEntry> {
    rows.iter()
        .map(|(schema, catalog)| SchemaEntry {
            schema: Some((*schema).to_string()),
            catalog: Some((*catalog).to_string()),
        })
        .collect()
}

fn four_columns() -> Vec<ColumnInfo> {
    columns(vec![
        column_row(JdbcType::Integer, 12, "testCol1"),
        column_row(JdbcType::Varchar, 25, "testCol2"),
        column_row(JdbcType::Timestamp, 93, "testCol3"),
        column_row(JdbcType::TimestampWithTimezone, 93, "testCol4"),
    ])
}

fn expected_four_column_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_field(FieldBuilder::new_builder("testCol1", DataType::Int32).build());
    builder.add_field(FieldBuilder::new_builder("testCol2", DataType::Utf8).build());
    builder.add_field(FieldBuilder::new_builder("testCol3", DataType::Date64).build());
    builder.add_field(FieldBuilder::new_builder("testCol4", DataType::Date64).build());
    builder.add_field(FieldBuilder::new_builder(PARTITION, DataType::Utf8).build());
    builder.build()
}

fn strings(labels: &[&str], rows: &[&[&str]]) -> ResultSetData {
    ResultSetData::of_strings(labels, rows)
}

fn count_rows(count: i64) -> ResultSetData {
    ResultSetData::of_columns(
        vec![ColumnMetadata::new("row_count", JdbcType::BigInt)],
        vec![vec![SqlValue::BigInt(count)]],
    )
}

fn counts_rows(count: i32) -> ResultSetData {
    ResultSetData::of_columns(
        vec![ColumnMetadata::new("COUNTS", JdbcType::Integer)],
        vec![vec![SqlValue::Integer(count)]],
    )
}

fn integration_rows(rows: &[&[&str]]) -> ResultSetData {
    strings(&["property", "property_value"], rows)
}

fn layout_table_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder
        .add_int_field("day")
        .add_int_field("month")
        .add_int_field("year")
        .add_string_field("preparedStmt")
        .add_string_field("queryId")
        .add_string_field(PARTITION);
    builder.build()
}

fn layout_request(
    identity: FederatedIdentity,
    table: TableName,
    schema: Schema,
    constraints: Constraints,
) -> GetTableLayoutRequest {
    GetTableLayoutRequest::new(
        Some(identity),
        Some("queryId".to_string()),
        "default",
        table,
        constraints,
        schema,
        vec![PARTITION.to_string()],
    )
}

fn partition_values(block: &Block, column: &str) -> Vec<Value> {
    let vector = block.get_field_vector(column).unwrap();
    (0..block.row_count())
        .map(|i| vector.get_object(i))
        .collect()
}

fn text_partitions(values: &[&str]) -> Block {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_string_field(PARTITION);
    let mut block = BlockAllocatorImpl::new().create_block_from_schema(&builder.build());
    for (row, value) in values.iter().enumerate() {
        block.set_value(PARTITION, row, *value).unwrap();
    }
    block.set_row_count(values.len());
    block
}

fn splits_request(
    identity: FederatedIdentity,
    partitions: Block,
    partition_cols: Vec<String>,
    constraints: Constraints,
) -> GetSplitsRequest {
    GetSplitsRequest::new(
        Some(identity),
        Some("queryId".to_string()),
        "catalog_name",
        TableName::new("schema", "table_name"),
        partitions,
        partition_cols,
        constraints,
        None,
    )
}

fn snowflake_error(message: &str) -> SqlError {
    SqlError::snowflake(message)
}

fn route_partitions(connection: &FakeConnection, count: i64, uniqueness: Result<i32, SqlError>) {
    let uniqueness = Arc::new(uniqueness);
    connection.on_query(move |statement: &PreparedStatement| {
        let sql = statement.sql();
        if sql.starts_with(snowflake_constants::SHOW_PRIMARY_KEYS_QUERY) {
            return Ok(strings(&["column_name"], &[&["id"]]));
        }
        if sql.contains("GROUP BY") {
            return match uniqueness.as_ref() {
                Ok(value) => Ok(counts_rows(*value)),
                Err(e) => Err(e.clone()),
            };
        }
        if sql.contains("information_schema.tables") {
            return Ok(count_rows(count));
        }
        Ok(ResultSetData::empty(&["TABLE_SCHEM", "TABLE_NAME"]))
    });
}

fn day_partition_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_int_field("day").add_string_field(PARTITION);
    builder.build()
}

fn connector_code(error: &FederationError) -> Option<FederationSourceErrorCode> {
    error.error_code()
}

#[tokio::test]
async fn do_get_table_no_columns() {
    let f = fixture();
    let error = f
        .handler
        .get_table(&table_request(TableName::new("testSchema", "testTable")))
        .await
        .unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::EntityNotFoundException),
        connector_code(&error)
    );
    assert_eq!("Could not find table in testSchema", error.to_string());
}

#[tokio::test]
async fn do_get_table_sql_exception() {
    let f = fixture();
    f.connection
        .meta
        .set_columns(Err(SqlError::new("columns failed")));
    let error = f
        .handler
        .get_table(&table_request(TableName::new("testSchema", "testTable")))
        .await
        .unwrap_err();
    let sql = SqlError::from_federation_error(&error).unwrap();
    assert_eq!("java.sql.SQLException", sql.java_class_name());
    assert_eq!("columns failed", sql.message());
}

#[tokio::test]
async fn do_get_table_exception() {
    let f = fixture();
    let error = f
        .handler
        .get_table(&table_request(TableName::new("testSchema", "test@schema")))
        .await
        .unwrap_err();
    assert!(error.is_runtime_exception());
}

#[tokio::test]
async fn do_get_table_no_columns_exception() {
    let f = fixture();
    let error = f
        .handler
        .get_table(&table_request(TableName::new("testSchema", "test@table")))
        .await
        .unwrap_err();
    assert!(error.is_runtime_exception());
    assert_eq!(
        Some(FederationSourceErrorCode::EntityNotFoundException),
        connector_code(&error)
    );
}

#[tokio::test]
async fn do_get_table() {
    let f = fixture();
    f.connection.meta.set_columns(Ok(four_columns()));
    f.connection.set_catalog(Some(CATALOG));
    let table = TableName::new("TESTSCHEMA", "TESTTABLE");
    let response = f
        .handler
        .get_table(&table_request(table.clone()))
        .await
        .unwrap();
    assert_eq!(&expected_four_column_schema(), response.schema());
    assert_eq!(&table, response.table_name());
    assert_eq!(CATALOG, response.catalog_name());
    let call = &f.connection.meta.get_columns_calls()[0];
    assert_eq!(Some(CATALOG.to_string()), call.catalog);
    assert_eq!(Some("TESTSCHEMA".to_string()), call.schema_pattern);
    assert_eq!(Some("TESTTABLE".to_string()), call.table_name_pattern);
    assert_eq!(None, call.column_name_pattern);
}

#[tokio::test]
async fn do_list_schema_names() {
    let f = fixture();
    f.connection.meta.set_schemas(Ok(schemas(&[
        ("TESTSCHEMA", CATALOG),
        ("TESTSCHEMA2", CATALOG),
    ])));
    f.connection.set_catalog(Some(CATALOG));
    let request = ListSchemasRequest::new(Some(identity()), Some("queryId".to_string()), CATALOG);
    let response = f.handler.list_schema_names(&request).await.unwrap();
    assert_eq!(2, response.schemas().len());
    assert!(response.schemas().iter().any(|s| s == "TESTSCHEMA"));
    assert!(response.schemas().iter().any(|s| s == "TESTSCHEMA2"));
}

#[tokio::test]
async fn get_partitions() {
    let f = fixture();
    f.connection
        .on_query(|_statement: &PreparedStatement| Ok(count_rows(1000)));
    let request = layout_request(
        identity(),
        TableName::new("schema1", "table1"),
        layout_table_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    assert!(response.partitions().row_count() > 0);
}

#[tokio::test]
async fn get_partitions_with_s3_export() {
    let f = fixture_with(FakeS3Lister::default(), export_options());
    let request = layout_request(
        identity(),
        TableName::new("schema1", "table1"),
        layout_table_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    let partitions = response.partitions();
    assert!(partitions.row_count() > 0);
    let reader = partitions.get_field_reader(S3_COLUMN).unwrap();
    reader.set_position(0);
    let bytes = reader.read_byte_array().unwrap();
    assert_eq!(layout_table_schema(), schema_from_bytes(&bytes).unwrap());
    assert!(f.connection.prepared_sql().is_empty());
}

#[tokio::test]
async fn do_get_splits() {
    let f = fixture();
    let mut builder = SchemaBuilder::new_builder();
    builder
        .add_int_field("day")
        .add_int_field("month")
        .add_int_field("year")
        .add_string_field("preparedStmt")
        .add_string_field("queryId")
        .add_string_field(PARTITION);
    let num_partitions = 10;
    let mut partitions = BlockAllocatorImpl::new().create_block_from_schema(&builder.build());
    for i in 0..num_partitions {
        let n = i32::try_from(i).unwrap();
        partitions.set_value("day", i, 2016 + n).unwrap();
        partitions.set_value("month", i, (n % 12) + 1).unwrap();
        partitions.set_value("year", i, (n % 28) + 1).unwrap();
        partitions
            .set_value("preparedStmt", i, "SELECT * FROM table")
            .unwrap();
        partitions.set_value("queryId", i, i.to_string()).unwrap();
        partitions
            .set_value(PARTITION, i, format!("partition_{i}"))
            .unwrap();
    }
    partitions.set_row_count(num_partitions);
    let request = splits_request(
        identity(),
        partitions,
        vec!["preparedStmt".to_string(), "queryId".to_string()],
        constraints(),
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(num_partitions, response.splits().len());
    for split in response.splits() {
        assert!(split.spill_location().is_some());
    }
    let mut partition_values: Vec<&str> = response
        .splits()
        .iter()
        .filter_map(|split| split.get_property(PARTITION))
        .collect();
    partition_values.sort_unstable();
    let mut expected: Vec<String> = (0..num_partitions)
        .map(|i| format!("partition_{i}"))
        .collect();
    expected.sort_unstable();
    assert_eq!(expected, partition_values);
    assert!(response.continuation_token().is_none());
    assert!(f.connection.executed().is_empty());
    assert!(f.lister.calls().is_empty());
}

#[tokio::test]
async fn do_get_splits_with_s3_export() {
    let f = fixture_with(
        FakeS3Lister::with_keys(&[
            "0/part_0.csv",
            "1/part_1.csv",
            "2/part_2.csv",
            "3/part_3.csv",
            "4/part_4.csv",
            "5/part_5.csv",
            "6/part_6.csv",
            "7/part_7.csv",
            "8/part_8.csv",
            "9/part_9.csv",
        ]),
        export_options(),
    );
    f.connection.on_query(|statement: &PreparedStatement| {
        if statement.sql() == DESC_TEST_INTEGRATION {
            return Ok(integration_rows(&[
                &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/export/"],
                &["STORAGE_PROVIDER", "S3"],
            ]));
        }
        Ok(ResultSetData::empty(&[]))
    });
    let schema = layout_table_schema();
    let mut builder = SchemaBuilder::new_builder();
    builder.add_field_with_type(S3_COLUMN, DataType::Binary);
    let mut partitions = BlockAllocatorImpl::new().create_block_from_schema(&builder.build());
    partitions
        .set_value(
            S3_COLUMN,
            0,
            schema_to_bytes(&schema, IpcFormat::ArrowDefault).unwrap(),
        )
        .unwrap();
    partitions.set_row_count(1);
    let request = splits_request(
        identity(),
        partitions,
        vec!["preparedStmt".to_string(), "queryId".to_string()],
        constraints(),
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(10, response.splits().len());
    for split in response.splits() {
        assert!(split.spill_location().is_some());
        assert_eq!(Some("test-bucket"), split.get_property("exportBucket"));
        assert_eq!(Some("queryId"), split.get_property("query_id"));
    }
    let executed = f.connection.executed();
    let copy = executed
        .iter()
        .find(|statement| statement.sql().starts_with("COPY INTO"))
        .unwrap();
    assert!(copy
        .sql()
        .starts_with("COPY INTO 's3://test-bucket/export/queryId/"));
    assert!(copy.sql().contains(
        "FROM (SELECT \"day\", \"month\", \"year\", \"preparedStmt\", \"queryId\" FROM \"schema\".\"table_name\") STORAGE_INTEGRATION = \"TEST_INTEGRATION\" HEADER = TRUE FILE_FORMAT = (TYPE = 'PARQUET', COMPRESSION = 'SNAPPY') MAX_FILE_SIZE = 52428800"
    ));
    let (bucket, prefix) = f.lister.calls()[0].clone();
    assert_eq!("test-bucket", bucket);
    assert!(prefix.starts_with("export/queryId/"));
    assert!(prefix.ends_with('/'));
}

#[tokio::test]
async fn do_get_splits_with_s3_export_keeps_format_text_in_query() {
    let f = fixture_with(FakeS3Lister::with_keys(&["0/part_0.csv"]), export_options());
    f.connection.on_query(|statement: &PreparedStatement| {
        if statement.sql() == DESC_TEST_INTEGRATION {
            return Ok(integration_rows(&[
                &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/export/"],
                &["STORAGE_PROVIDER", "S3"],
            ]));
        }
        Ok(ResultSetData::empty(&[]))
    });
    let request = splits_request(
        identity(),
        BlockAllocatorImpl::new().create_block_from_schema(&SchemaBuilder::new_builder().build()),
        Vec::new(),
        qpt_constraints("SELECT * FROM t WHERE c = '%s'"),
    );
    f.handler.get_splits(&request).await.unwrap();
    let executed = f.connection.executed();
    let copy = executed
        .iter()
        .find(|statement| statement.sql().starts_with("COPY INTO"))
        .unwrap();
    assert!(copy.sql().contains(
        "FROM (SELECT * FROM t WHERE c = '%s') STORAGE_INTEGRATION = \"TEST_INTEGRATION\" HEADER = TRUE"
    ));
}

#[tokio::test]
async fn test_get_partition_schema() {
    let f = fixture();
    let schema = f.handler.partition_schema(CATALOG).unwrap();
    assert_eq!(1, schema.fields().len());
    assert_eq!(PARTITION, schema.field(0).name());
    assert_eq!(&DataType::Utf8, schema.field(0).data_type());
}

#[tokio::test]
async fn test_list_database_names() {
    let f = fixture();
    f.connection.meta.set_schemas(Ok(schemas(&[
        ("schema1", CATALOG),
        ("information_schema", CATALOG),
        ("schema2", CATALOG),
    ])));
    f.connection.set_catalog(Some(CATALOG));
    let names = f.handler.list_database_names(&f.connection).await.unwrap();
    assert_eq!(vec!["schema1".to_string(), "schema2".to_string()], names);
}

#[tokio::test]
async fn test_get_schema_with_data_types() {
    let f = fixture();
    f.connection.meta.set_columns(Ok(columns(vec![
        column_row(JdbcType::Integer, 10, "intCol"),
        column_row(JdbcType::Varchar, 255, "varcharCol"),
        column_row(JdbcType::Timestamp, 0, "timestampCol"),
        column_row(JdbcType::TimestampWithTimezone, 0, "timestampTzCol"),
    ])));
    let schema = f
        .handler
        .schema(
            &f.connection,
            &TableName::new("testSchema", "testTable"),
            &SchemaBuilder::new_builder().build(),
        )
        .await
        .unwrap();
    assert_eq!(4, schema.fields().len());
    assert_eq!(
        &DataType::Int32,
        schema.field_with_name("intCol").unwrap().data_type()
    );
    assert_eq!(
        &DataType::Utf8,
        schema.field_with_name("varcharCol").unwrap().data_type()
    );
    assert_eq!(
        &DataType::Date64,
        schema.field_with_name("timestampCol").unwrap().data_type()
    );
    assert_eq!(
        &DataType::Date64,
        schema
            .field_with_name("timestampTzCol")
            .unwrap()
            .data_type()
    );
}

#[tokio::test]
async fn test_get_schema_no_matching_columns() {
    let f = fixture();
    f.connection.meta.set_columns(Ok(columns(Vec::new())));
    let error = f
        .handler
        .schema(
            &f.connection,
            &TableName::new("testSchema", "testTable"),
            &SchemaBuilder::new_builder().build(),
        )
        .await
        .unwrap_err();
    assert!(error.is_runtime_exception());
    assert_eq!("Could not find table in testSchema", error.to_string());
}

#[tokio::test]
async fn test_get_schema_unsupported_types() {
    let f = fixture();
    f.connection.meta.set_columns(Ok(columns(vec![
        column_row(JdbcType::Array, 0, "arrayCol"),
        column_row(JdbcType::Integer, 10, "intCol"),
    ])));
    let schema = f
        .handler
        .schema(
            &f.connection,
            &TableName::new("testSchema", "testTable"),
            &SchemaBuilder::new_builder().build(),
        )
        .await
        .unwrap();
    assert_eq!(2, schema.fields().len());
    assert!(matches!(
        schema.field_with_name("arrayCol").unwrap().data_type(),
        DataType::List(_)
    ));
    assert_eq!(
        &DataType::Int32,
        schema.field_with_name("intCol").unwrap().data_type()
    );
}

#[tokio::test]
async fn test_get_partitions_for_view() {
    let f = fixture();
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(strings(
            &["TABLE_SCHEMA", "TABLE_NAME"],
            &[&["testSchema", "testView"]],
        ))
    });
    let mut builder = SchemaBuilder::new_builder();
    builder.add_int_field("col1").add_string_field(PARTITION);
    let request = layout_request(
        identity(),
        TableName::new("testSchema", "testView"),
        builder.build(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    assert_eq!(1, response.partitions().row_count());
    assert_eq!(
        vec![Value::String("*".to_string())],
        partition_values(response.partitions(), PARTITION)
    );
}

#[tokio::test]
async fn test_getlist_exported_objects_s3_path() {
    let f = fixture_with(
        FakeS3Lister::with_keys(&["queryId123/file1.parquet", "queryId123/file2.parquet"]),
        base_options(),
    );
    let result = f
        .handler
        .dialect()
        .get_list_exported_objects("test-bucket", "queryId123")
        .await
        .unwrap();
    assert_eq!(2, result.len());
    assert_eq!("queryId123/file1.parquet", result[0].key());
    assert_eq!("queryId123/file2.parquet", result[1].key());
    assert_eq!(
        vec![("test-bucket".to_string(), "queryId123".to_string())],
        f.lister.calls()
    );
}

#[tokio::test]
async fn test_getlist_exported_objects_s3_exception() {
    let f = fixture_with(FakeS3Lister::failing("Access denied"), base_options());
    let error = f
        .handler
        .dialect()
        .get_list_exported_objects("test-bucket", "queryId123")
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::Runtime { .. }));
    assert_eq!(
        "Failed to list objects in bucket test-bucket with prefix queryId123",
        error.to_string()
    );
}

#[tokio::test]
async fn test_get_sf_storage_integration_name_from_config() {
    let f = fixture_with(
        FakeS3Lister::default(),
        options_with(&[(
            snowflake_constants::STORAGE_INTEGRATION_CONFIG_KEY,
            "TEST_INTEGRATION",
        )]),
    );
    assert_eq!(
        Some("TEST_INTEGRATION"),
        SnowflakeMetadataDialect::get_sf_storage_integration_name_from_config(
            f.handler.config_options()
        )
    );
}

#[tokio::test]
async fn test_get_data_source_capabilities() {
    let f = fixture();
    let request = GetDataSourceCapabilitiesRequest::new(
        Some(identity()),
        Some("queryId".to_string()),
        CATALOG,
    );
    let response = f
        .handler
        .get_data_source_capabilities(&request)
        .await
        .unwrap();
    let capabilities = response.capabilities();
    let names: Vec<&str> = capabilities.keys().map(String::as_str).collect();
    assert_eq!(
        vec![
            "supports_filter_pushdown",
            "supports_limit_pushdown",
            "supports_complex_expression_pushdown",
            "supports_top_n_pushdown",
            "SYSTEM.QUERY",
        ],
        names
    );
}

#[tokio::test]
async fn test_enhance_partition_schema() {
    let f = fixture();
    let mut builder = SchemaBuilder::new_builder();
    let request = layout_request(
        identity(),
        TableName::new("testSchema", "testTable"),
        SchemaBuilder::new_builder().build(),
        constraints(),
    );
    f.handler.enhance_partition_schema(&mut builder, &request);
    assert!(builder.get_field(PARTITION).is_some());
}

#[tokio::test]
async fn test_get_storage_integration_s3_path_from_snow_flake() {
    let f = fixture();
    f.connection.on_query(|statement: &PreparedStatement| {
        assert!(statement.sql().contains("DESC STORAGE INTEGRATION"));
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/path/"],
            &["STORAGE_PROVIDER", "S3"],
        ]))
    });
    let result = f
        .handler
        .dialect()
        .get_storage_integration_s3_path_from_snow_flake(&f.connection, "TEST_INTEGRATION")
        .await
        .unwrap();
    assert_eq!("s3://test-bucket/path", result);
}

#[tokio::test]
async fn test_get_storage_integration_s3_path_invalid_provider() {
    let f = fixture();
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/path/"],
            &["STORAGE_PROVIDER", "AZURE"],
        ]))
    });
    let error = f
        .handler
        .dialect()
        .get_storage_integration_s3_path_from_snow_flake(&f.connection, "TEST_INTEGRATION")
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::IllegalArgument(_)));
    assert_eq!(
        "Snowflake Storage Integration, field:STORAGE_PROVIDER must be S3",
        error.to_string()
    );
}

#[tokio::test]
async fn test_get_storage_integration_s3_path_multiple_paths() {
    let f = fixture();
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://bucket1/, s3://bucket2/"],
            &["STORAGE_PROVIDER", "S3"],
        ]))
    });
    let error = f
        .handler
        .dialect()
        .get_storage_integration_s3_path_from_snow_flake(&f.connection, "TEST_INTEGRATION")
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::IllegalArgument(_)));
    assert_eq!(
        "Snowflake Storage Integration, field:STORAGE_ALLOWED_LOCATIONS must be a single S3 path",
        error.to_string()
    );
}

#[tokio::test]
async fn test_list_paginated_tables() {
    let f = fixture();
    f.connection.on_query(|statement: &PreparedStatement| {
        assert!(statement.sql().contains("LIMIT ? OFFSET ?"));
        Ok(strings(
            &["TABLE_NAME", "TABLE_SCHEM"],
            &[
                &["table1", "testSchema"],
                &["table2", "testSchema"],
                &["table3", "testSchema"],
            ],
        ))
    });
    let request = ListTablesRequest::new(
        Some(identity()),
        Some("queryId".to_string()),
        CATALOG,
        Some("testSchema".to_string()),
        None,
        10,
    );
    let response = f
        .handler
        .list_paginated_tables(&f.connection, &request)
        .await
        .unwrap();
    assert_eq!(3, response.tables().len());
    assert_eq!(None, response.next_token());
    let statement = &f.connection.executed()[0];
    assert_eq!(
        &[
            SqlValue::String("testSchema".to_string()),
            SqlValue::Integer(10),
            SqlValue::Integer(0),
        ],
        statement.parameters()
    );
}

#[tokio::test]
async fn test_get_paginated_tables() {
    let f = fixture();
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(strings(
            &["TABLE_NAME", "TABLE_SCHEM"],
            &[&["table1", "testSchema"], &["table2", "testSchema"]],
        ))
    });
    let tables = f
        .handler
        .dialect()
        .get_paginated_tables(&f.connection, "testSchema", 0, 10)
        .await
        .unwrap();
    assert_eq!(2, tables.len());
    assert_eq!("table1", tables[0].table_name());
    assert_eq!("table2", tables[1].table_name());
    assert_eq!(
        vec![snowflake_constants::LIST_PAGINATED_TABLES_QUERY.to_string()],
        f.connection.prepared_sql()
    );
}

#[tokio::test]
async fn test_handle_s3_export_splits_empty_objects() {
    let f = fixture_with(FakeS3Lister::default(), export_options());
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/"],
            &["STORAGE_PROVIDER", "S3"],
        ]))
    });
    let mut table_builder = SchemaBuilder::new_builder();
    table_builder
        .add_string_field("col1")
        .add_string_field("col2");
    let mut partition_builder = SchemaBuilder::new_builder();
    partition_builder
        .add_string_field("col1")
        .add_field_with_type(S3_COLUMN, DataType::Binary);
    let mut partitions =
        BlockAllocatorImpl::new().create_block_from_schema(&partition_builder.build());
    partitions
        .set_value(
            S3_COLUMN,
            0,
            schema_to_bytes(&table_builder.build(), IpcFormat::ArrowDefault).unwrap(),
        )
        .unwrap();
    partitions.set_row_count(1);
    let request = GetSplitsRequest::new(
        Some(identity()),
        Some("queryId".to_string()),
        CATALOG,
        TableName::new("testSchema", "testTable"),
        partitions,
        Vec::new(),
        constraints(),
        None,
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(1, response.splits().len());
    let split = &response.splits()[0];
    assert_eq!(Some(""), split.get_property("s3ObjectKey"));
    assert_eq!(Some("test-bucket"), split.get_property("exportBucket"));
    let copy = f.connection.executed().last().unwrap().sql().to_string();
    assert!(copy.contains("FROM (SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\")"));
}

#[tokio::test]
async fn test_handle_s3_export_splits_escapes_export_path_and_integration_name() {
    let f = fixture_with(
        FakeS3Lister::default(),
        options_with(&[
            (snowflake_constants::SNOWFLAKE_ENABLE_S3_EXPORT, "true"),
            (
                snowflake_constants::STORAGE_INTEGRATION_CONFIG_KEY,
                "test\"integration",
            ),
        ]),
    );
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/"],
            &["STORAGE_PROVIDER", "S3"],
        ]))
    });
    let mut table_builder = SchemaBuilder::new_builder();
    table_builder
        .add_string_field("col1")
        .add_string_field("col2");
    let mut partition_builder = SchemaBuilder::new_builder();
    partition_builder
        .add_string_field("col1")
        .add_field_with_type(S3_COLUMN, DataType::Binary);
    let mut partitions =
        BlockAllocatorImpl::new().create_block_from_schema(&partition_builder.build());
    partitions
        .set_value(
            S3_COLUMN,
            0,
            schema_to_bytes(&table_builder.build(), IpcFormat::ArrowDefault).unwrap(),
        )
        .unwrap();
    partitions.set_row_count(1);
    let request = GetSplitsRequest::new(
        Some(identity()),
        Some("query'id".to_string()),
        CATALOG,
        TableName::new("testSchema", "testTable"),
        partitions,
        Vec::new(),
        constraints(),
        None,
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(1, response.splits().len());
    let executed: Vec<String> = f
        .connection
        .executed()
        .into_iter()
        .map(|statement| statement.sql().to_string())
        .collect();
    assert!(executed.contains(&"DESC STORAGE INTEGRATION \"TEST\"\"INTEGRATION\"".to_string()));
    let copy = executed.last().unwrap();
    assert!(copy.starts_with("COPY INTO 's3://test-bucket/query''id/"));
    assert!(copy.contains(" STORAGE_INTEGRATION = \"test\"\"integration\" HEADER = TRUE"));
}

#[tokio::test]
async fn test_enhance_partition_schema_query_passthrough() {
    let f = fixture();
    let mut builder = SchemaBuilder::new_builder();
    let request = layout_request(
        identity(),
        TableName::new("testSchema", "testTable"),
        SchemaBuilder::new_builder().build(),
        qpt_constraints("SELECT * FROM custom_table"),
    );
    f.handler.enhance_partition_schema(&mut builder, &request);
    assert_eq!(0, builder.build().fields().len());
}

#[tokio::test]
async fn test_get_sf_storage_integration_name_from_config_empty() {
    let f = fixture();
    assert_eq!(
        None,
        SnowflakeMetadataDialect::get_sf_storage_integration_name_from_config(
            f.handler.config_options()
        )
    );
}

#[tokio::test]
async fn test_get_credential_provider_without_secret() {
    let f = fixture();
    assert!(f
        .handler
        .dialect()
        .credentials_provider(
            f.handler.context().handler(),
            match f.handler.connection() {
                ConnectionSource::Direct { config, .. } => config.secret(),
                ConnectionSource::Delegating => None,
            },
            None
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_get_storage_integration_properties() {
    let f = fixture();
    f.connection.on_query(|statement: &PreparedStatement| {
        assert_eq!(DESC_TEST_INTEGRATION, statement.sql());
        Ok(integration_rows(&[
            &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/path/"],
            &["STORAGE_PROVIDER", "S3"],
            &["ENABLED", "true"],
        ]))
    });
    let properties = f
        .handler
        .dialect()
        .get_storage_integration_properties(&f.connection, "TEST_INTEGRATION")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(3, properties.len());
    assert_eq!(
        "s3://test-bucket/path/",
        properties["STORAGE_ALLOWED_LOCATIONS"]
    );
    assert_eq!("S3", properties["STORAGE_PROVIDER"]);
    assert_eq!("true", properties["ENABLED"]);
}

#[tokio::test]
async fn test_get_storage_integration_properties_not_found() {
    let f = fixture();
    f.connection.on_query(|statement: &PreparedStatement| {
        assert_eq!(
            "DESC STORAGE INTEGRATION \"NONEXISTENT_INTEGRATION\"",
            statement.sql()
        );
        Err(SqlError::new(
            "Integration does not exist or not authorized",
        ))
    });
    let properties = f
        .handler
        .dialect()
        .get_storage_integration_properties(&f.connection, "NONEXISTENT_INTEGRATION")
        .await
        .unwrap();
    assert!(properties.is_none());
}

#[tokio::test]
async fn test_get_storage_integration_properties_sql_exception() {
    let f = fixture();
    f.connection
        .on_query(|_statement: &PreparedStatement| Err(SqlError::new("Database connection error")));
    let error = f
        .handler
        .dialect()
        .get_storage_integration_properties(&f.connection, "TEST_INTEGRATION")
        .await
        .unwrap_err();
    let sql = SqlError::from_federation_error(&error).unwrap();
    assert_eq!("Database connection error", sql.message());
}

#[tokio::test]
async fn test_do_get_table_with_annotation() {
    let f = casing_fixture("ANNOTATION");
    f.connection.meta.set_columns(Ok(four_columns()));
    f.connection.set_catalog(Some(CATALOG));
    let response = f
        .handler
        .get_table(&table_request(TableName::new(
            "testSchema",
            "testTable@schemaCase=upper&tableCase=lower",
        )))
        .await
        .unwrap();
    assert_eq!(&expected_four_column_schema(), response.schema());
    assert_eq!(
        &TableName::new("TESTSCHEMA", "testtable"),
        response.table_name()
    );
    assert_eq!(CATALOG, response.catalog_name());
    let call = &f.connection.meta.get_columns_calls()[0];
    assert_eq!(Some("TESTSCHEMA".to_string()), call.schema_pattern);
    assert_eq!(Some("testtable".to_string()), call.table_name_pattern);
}

async fn assert_view_check_binds_table_name(
    config_options: HashMap<String, Option<String>>,
    expected_schema: &str,
    expected_table: &str,
) {
    let f = fixture();
    f.connection.on_query(|_statement: &PreparedStatement| {
        Ok(strings(
            &["TABLE_SCHEMA", "TABLE_NAME"],
            &[&["TESTSCHEMA", "TESTTABLE"]],
        ))
    });
    let request = layout_request(
        identity_with(config_options),
        TableName::new("testschema", "testtable"),
        day_partition_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    assert!(response.partitions().row_count() > 0);
    assert_eq!(
        Value::String("*".to_string()),
        partition_values(response.partitions(), PARTITION)[0]
    );
    let view_check = &f.connection.executed()[0];
    assert_eq!(snowflake_constants::VIEW_CHECK_QUERY, view_check.sql());
    assert_eq!(
        &[
            SqlValue::String(expected_schema.to_string()),
            SqlValue::String(expected_table.to_string()),
        ],
        view_check.parameters()
    );
}

#[tokio::test]
async fn test_get_partitions_with_uppercase_casing_filter() {
    assert_view_check_binds_table_name(
        HashMap::from([(
            environment_constants::CATALOG_CASING_FILTER.to_string(),
            Some(environment_constants::UPPERCASE_ONLY.to_string()),
        )]),
        "TESTSCHEMA",
        "TESTTABLE",
    )
    .await;
}

#[tokio::test]
async fn test_get_partitions_without_casing_filter() {
    assert_view_check_binds_table_name(HashMap::new(), "testschema", "testtable").await;
}

#[tokio::test]
async fn test_get_splits_with_catalog_casing_filter() {
    let f = fixture();
    let request = splits_request(
        identity_with(HashMap::from([(
            environment_constants::CATALOG_CASING_FILTER.to_string(),
            Some(environment_constants::UPPERCASE_ONLY.to_string()),
        )])),
        text_partitions(&["partition-primary-\"id\"-limit-100-offset-0"]),
        Vec::new(),
        constraints(),
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(1, response.splits().len());
    let split = &response.splits()[0];
    assert_eq!(
        Some(environment_constants::UPPERCASE_ONLY),
        split.get_property(environment_constants::CATALOG_CASING_FILTER)
    );
    assert_eq!(
        Some("partition-primary-\"id\"-limit-100-offset-0"),
        split.get_property(PARTITION)
    );
    assert_eq!(None, response.continuation_token());
}

#[tokio::test]
async fn test_get_splits_without_catalog_casing_filter() {
    let f = fixture();
    let request = splits_request(
        identity(),
        text_partitions(&["partition-primary-\"id\"-limit-100-offset-0"]),
        Vec::new(),
        constraints(),
    );
    let response = f.handler.get_splits(&request).await.unwrap();
    assert_eq!(1, response.splits().len());
    assert_eq!(
        None,
        response.splits()[0].get_property(environment_constants::CATALOG_CASING_FILTER)
    );
}

#[tokio::test]
async fn test_do_get_table_with_none_casing() {
    let f = casing_fixture("NONE");
    f.connection.meta.set_columns(Ok(four_columns()));
    f.connection.set_catalog(Some(CATALOG));
    let table = TableName::new("TestSchema", "TestTable");
    let response = f
        .handler
        .get_table(&table_request(table.clone()))
        .await
        .unwrap();
    assert_eq!(&expected_four_column_schema(), response.schema());
    assert_eq!(&table, response.table_name());
    let call = &f.connection.meta.get_columns_calls()[0];
    assert_eq!(Some("TestSchema".to_string()), call.schema_pattern);
    assert_eq!(Some("TestTable".to_string()), call.table_name_pattern);
}

#[tokio::test]
async fn get_partitions_table_above_large_threshold_skips_uniqueness_check() {
    let f = fixture();
    route_partitions(
        &f.connection,
        snowflake_constants::LARGE_TABLE_THRESHOLD + 1,
        Ok(1),
    );
    let request = layout_request(
        identity(),
        TableName::new("schema1", "table1"),
        day_partition_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    let prepared = f.connection.prepared_sql();
    assert!(!prepared
        .iter()
        .any(|sql| sql.starts_with(snowflake_constants::SHOW_PRIMARY_KEYS_QUERY)));
    assert!(!prepared.iter().any(|sql| sql.contains("GROUP BY")));
    assert_eq!(1, response.partitions().row_count());
    assert_eq!(
        Value::String("partition-primary--limit-100000001-offset-0".to_string()),
        partition_values(response.partitions(), PARTITION)[0]
    );
}

#[tokio::test]
async fn get_partitions_table_below_threshold_applies_uniqueness_check_timeout() {
    let f = fixture();
    route_partitions(&f.connection, 1_000_000, Ok(1));
    let request = layout_request(
        identity(),
        TableName::new("schema1", "table1"),
        day_partition_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    let uniqueness = f
        .connection
        .executed()
        .into_iter()
        .find(|statement| statement.sql().contains("GROUP BY"))
        .unwrap();
    assert_eq!(
        snowflake_constants::UNIQUENESS_CHECK_TIMEOUT_SECONDS,
        uniqueness.query_timeout()
    );
    assert_eq!(
        "SELECT \"id\", count(*) as COUNTS FROM \"schema1\".\"table1\" GROUP BY \"id\" ORDER BY COUNTS DESC LIMIT 1",
        uniqueness.sql()
    );
    let values = partition_values(response.partitions(), PARTITION);
    assert_eq!(50, values.len());
    assert_eq!(
        Value::String("partition-primary-\"id\"-limit-20000-offset-0".to_string()),
        values[0]
    );
    assert_eq!(
        Value::String("partition-primary-\"id\"-limit-20000-offset-980000".to_string()),
        values[49]
    );
}

#[tokio::test]
async fn get_partitions_uniqueness_check_times_out_falls_back_to_single_partition() {
    let f = fixture();
    route_partitions(
        &f.connection,
        1_000_000,
        Err(SqlError::new("statement timed out")),
    );
    let request = layout_request(
        identity(),
        TableName::new("schema1", "table1"),
        day_partition_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    assert_eq!(1, response.partitions().row_count());
    assert_eq!(
        Value::String("partition-primary--limit-1000000-offset-0".to_string()),
        partition_values(response.partitions(), PARTITION)[0]
    );
}

#[tokio::test]
async fn create_credentials_provider_returns_provider_only_when_secret_present() {
    let f = fixture();
    assert!(create_snowflake_credentials_provider(
        f.handler.context().handler(),
        Some("testSecret"),
        None
    )
    .is_some());
    assert!(
        create_snowflake_credentials_provider(f.handler.context().handler(), Some(""), None)
            .is_none()
    );
    assert!(
        create_snowflake_credentials_provider(f.handler.context().handler(), None, None).is_none()
    );
}

#[tokio::test]
async fn create_credentials_provider_passes_request_override_configuration() {
    let f = fixture();
    let override_config = AwsRequestOverrideConfiguration::builder().build();
    let provider = create_snowflake_credentials_provider(
        f.handler.context().handler(),
        Some("testSecret"),
        Some(&override_config),
    )
    .unwrap();
    assert_eq!("testSecret", provider.oauth_secret_name());
    assert!(provider.request_override_configuration().is_some());
    let boxed = create_credentials_provider(
        f.handler.context().handler(),
        "testSecret",
        Some(&override_config),
    )
    .unwrap();
    assert!(boxed
        .as_any()
        .is::<athena_snowflake::SnowflakeCredentialsProvider>());
}

#[tokio::test]
async fn export_split_error_mapping() {
    let failing_copy = |message: &'static str| {
        let f = fixture_with(FakeS3Lister::default(), export_options());
        f.connection.on_query(|_statement: &PreparedStatement| {
            Ok(integration_rows(&[
                &["STORAGE_ALLOWED_LOCATIONS", "s3://test-bucket/"],
                &["STORAGE_PROVIDER", "S3"],
            ]))
        });
        f.connection
            .on_execute(move |_statement: &PreparedStatement| Err(snowflake_error(message)));
        f
    };
    let mut partition_builder = SchemaBuilder::new_builder();
    partition_builder.add_field_with_type(S3_COLUMN, DataType::Binary);
    let partition_schema = partition_builder.build();
    let table_schema = Schema::new(vec![Field::new("c", DataType::Utf8, true)]);
    let make_request = || {
        let mut partitions = BlockAllocatorImpl::new().create_block_from_schema(&partition_schema);
        partitions
            .set_value(
                S3_COLUMN,
                0,
                schema_to_bytes(&table_schema, IpcFormat::ArrowDefault).unwrap(),
            )
            .unwrap();
        partitions.set_row_count(1);
        splits_request(identity(), partitions, Vec::new(), constraints())
    };
    let existing = failing_copy("Files already existing at the unload destination");
    let response = existing.handler.get_splits(&make_request()).await.unwrap();
    assert_eq!(1, response.splits().len());
    let denied = failing_copy("Insufficient privileges");
    let error = denied
        .handler
        .get_splits(&make_request())
        .await
        .unwrap_err();
    assert_eq!(
        "Exception in execution export statement Insufficient privileges",
        error.to_string()
    );
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException),
        error.error_code()
    );
    let missing = fixture_with(
        FakeS3Lister::default(),
        options_with(&[(snowflake_constants::SNOWFLAKE_ENABLE_S3_EXPORT, "true")]),
    );
    let error = missing
        .handler
        .get_splits(&make_request())
        .await
        .unwrap_err();
    assert_eq!(
        "Exception in execution export statement :Snowflake storage integration name not found",
        error.to_string()
    );
    let calls = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&calls);
    let not_found = fixture_with(FakeS3Lister::default(), export_options());
    not_found
        .connection
        .on_query(move |_statement: &PreparedStatement| {
            *counter.lock().unwrap() += 1;
            Err(SqlError::new(
                "Integration TEST_INTEGRATION does not exist or not authorized",
            ))
        });
    let error = not_found
        .handler
        .get_splits(&make_request())
        .await
        .unwrap_err();
    assert_eq!(
        "Exception in execution export statement :Snowflake Storage Integration: name:TEST_INTEGRATION not found",
        error.to_string()
    );
    assert_eq!(1, *calls.lock().unwrap());
}

#[tokio::test]
async fn get_schema_maps_snowflake_errors_to_access_denied() {
    let f = fixture();
    f.connection.meta.set_columns(Err(snowflake_error(
        "Object does not exist or not authorized.",
    )));
    let error = f
        .handler
        .schema(
            &f.connection,
            &TableName::new("testSchema", "testTable"),
            &SchemaBuilder::new_builder().build(),
        )
        .await
        .unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::AccessDeniedException),
        error.error_code()
    );
    assert_eq!(
        "Object does not exist or not authorized.",
        error.to_string()
    );
}

#[test]
fn partition_values_follow_java_arithmetic() {
    assert_eq!(
        vec!["partition-primary-\"id\"-limit-9-offset-0".to_string()],
        athena_snowflake::partition_values(9, Some("\"id\""))
    );
    let values = athena_snowflake::partition_values(10_001, Some("\"id\""));
    assert_eq!(50, values.len());
    assert_eq!("partition-primary-\"id\"-limit-201-offset-0", values[0]);
    assert_eq!("partition-primary-\"id\"-limit-201-offset-9849", values[49]);
}

#[tokio::test]
async fn get_partitions_escapes_quotes_in_primary_key_and_table_identifiers() {
    let f = fixture();
    f.connection.on_query(|statement: &PreparedStatement| {
        let sql = statement.sql();
        if sql.starts_with(snowflake_constants::SHOW_PRIMARY_KEYS_QUERY) {
            return Ok(strings(&["column_name"], &[&["id\"x"]]));
        }
        if sql.contains("GROUP BY") {
            return Ok(counts_rows(1));
        }
        if sql.contains("information_schema.tables") {
            return Ok(count_rows(1_000_000));
        }
        Ok(ResultSetData::empty(&["TABLE_SCHEM", "TABLE_NAME"]))
    });
    let request = layout_request(
        identity(),
        TableName::new("sch\"ema", "ta\"ble"),
        day_partition_schema(),
        constraints(),
    );
    let response = f
        .handler
        .get_table_layout(f.allocator.as_ref(), &request)
        .await
        .unwrap();
    let executed: Vec<String> = f
        .connection
        .executed()
        .into_iter()
        .map(|statement| statement.sql().to_string())
        .collect();
    assert!(executed.contains(&"SHOW PRIMARY KEYS IN \"sch\"\"ema\".\"ta\"\"ble\"".to_string()));
    assert!(executed.contains(
        &"SELECT \"id\"\"x\", count(*) as COUNTS FROM \"sch\"\"ema\".\"ta\"\"ble\" GROUP BY \"id\"\"x\" ORDER BY COUNTS DESC LIMIT 1"
            .to_string()
    ));
    assert_eq!(
        Value::String("partition-primary-\"id\"\"x\"-limit-20000-offset-0".to_string()),
        partition_values(response.partitions(), PARTITION)[0]
    );
}
