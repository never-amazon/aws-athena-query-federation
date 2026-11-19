mod snowflake_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::{
    ArrayRef, BooleanArray, Date32Array, Decimal128Array, Float32Array, Float64Array, Int16Array,
    Int64Array, Int8Array, StringArray, TimestampMillisecondArray,
};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, FieldVector, S3BlockSpillReader, SchemaBuilder,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{HandlerContext, RecordHandler};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, EncryptionKey, FederatedIdentity, KeySource,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{RecordDialect, SplitQuery, EXPORT_SCHEMA};
use athena_jdbc::qpt::QUERY;
use athena_jdbc::sql::{ColumnMetadata, JdbcType, PreparedStatement, ResultSetMetaData, SqlError};
use athena_snowflake::{
    construct_s3_uri, convert_timestamp_tz_milli_to_date_milli_fast, snowflake_constants,
    ExportReader, S3ParquetExportReader, SnowflakeCredentialsProvider, SnowflakeRecordHandler,
};
use athena_snowflake::{create_snowflake_credentials_provider, SnowflakeRecordDialect};
use bigdecimal::BigDecimal;
use indexmap::IndexMap;
use parquet::arrow::ArrowWriter;
use snowflake_support::{
    CountingSpiller, FakeConnection, FakeConnectionFactory, FakeExportReader, FakeSecretsManager,
    Fakes, ResultSetData,
};

const EXPORT_QUERY: &str = "SELECT id FROM t";
const QUERY_ID: &str = "query_id";
const DEFAULT_CATALOG: &str = "default";
const CONNECTION_STRING: &str = "snowflake://jdbc:snowflake://hostname/?warehouse=warehousename&db=dbname&schema=schemaname&user=xxx&password=xxx";

struct Fixture {
    fakes: Fakes,
    connection: FakeConnection,
    handler: SnowflakeRecordHandler,
    allocator: Arc<dyn BlockAllocator>,
}

fn options(export: bool) -> HashMap<String, String> {
    let mut options =
        HashMap::from([("disable_spill_encryption".to_string(), "false".to_string())]);
    if export {
        options.insert(
            snowflake_constants::SNOWFLAKE_ENABLE_S3_EXPORT.to_string(),
            "true".to_string(),
        );
    }
    options
}

fn fixture_with(
    config: DatabaseConnectionConfig,
    export: bool,
    reader: Option<FakeExportReader>,
) -> Fixture {
    let fakes = Fakes::with_secrets(FakeSecretsManager::with_secret(
        "testSecret",
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ));
    let connection = FakeConnection::new();
    connection.meta.set_database_product_name("Snowflake");
    let factory = Arc::new(FakeConnectionFactory::new(connection.clone()));
    let clients = fakes.clients();
    let export_reader: Arc<dyn ExportReader> = match reader {
        Some(reader) => Arc::new(reader),
        None => Arc::new(S3ParquetExportReader::new(Arc::clone(&clients.s3))),
    };
    let context = HandlerContext::new(config.engine(), clients, options(export)).unwrap();
    let handler = SnowflakeRecordDialect::handler(context, config, factory, export_reader);
    Fixture {
        fakes,
        connection,
        handler,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new(
        "testCatalog",
        snowflake_constants::SNOWFLAKE_NAME,
        CONNECTION_STRING,
    )
    .unwrap()
}

fn fixture(export: bool, reader: Option<FakeExportReader>) -> Fixture {
    fixture_with(config(), export, reader)
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
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

fn spill_location() -> S3SpillLocation {
    S3SpillLocation::new_builder()
        .with_bucket("spill-bucket")
        .with_prefix("prefix")
        .with_query_id("spill-query")
        .with_split_id("spill-split")
        .build()
}

async fn key() -> EncryptionKey {
    KeySource::Local.create(None).await.unwrap()
}

async fn export_split(bucket: Option<&str>, object_key: Option<&str>) -> Split {
    let mut builder = Split::new_builder(Some(spill_location().into()), Some(key().await))
        .add(snowflake_constants::SNOWFLAKE_SPLIT_QUERY_ID, QUERY_ID);
    if let Some(bucket) = bucket {
        builder = builder.add(snowflake_constants::SNOWFLAKE_SPLIT_EXPORT_BUCKET, bucket);
    }
    if let Some(object_key) = object_key {
        builder = builder.add(snowflake_constants::SNOWFLAKE_SPLIT_OBJECT_KEY, object_key);
    }
    builder.build()
}

async fn partition_split(partition: &str) -> Split {
    Split::new_builder(Some(spill_location().into()), Some(key().await))
        .add("partition", partition)
        .build()
}

fn id_name_schema() -> Schema {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_big_int_field("id").add_string_field("name");
    builder.build()
}

fn request(
    schema: Schema,
    split: Split,
    constraints: Constraints,
    max_block: i64,
    max_inline: i64,
) -> ReadRecordsRequest {
    ReadRecordsRequest::new(
        Some(identity()),
        DEFAULT_CATALOG,
        QUERY_ID,
        TableName::new("default", "test_table"),
        schema,
        split,
        constraints,
        max_block,
        max_inline,
    )
}

fn root_batch() -> RecordBatch {
    let schema = Schema::new(vec![
        Field::new("bitField", DataType::Boolean, true),
        Field::new("tinyIntField", DataType::Int8, true),
        Field::new("smallIntField", DataType::Int16, true),
        Field::new("day", DataType::Int64, true),
        Field::new("month", DataType::Int64, true),
        Field::new("year", DataType::Int64, true),
        Field::new("float4Field", DataType::Float32, true),
        Field::new("float8Field", DataType::Float64, true),
        Field::new("decimalField", DataType::Decimal128(38, 10), true),
        Field::new("dateDayField", DataType::Date32, true),
        Field::new("preparedStmt", DataType::Utf8, true),
        Field::new("queryId", DataType::Utf8, true),
        Field::new("awsRegionSql", DataType::Utf8, true),
    ]);
    let decimal = Decimal128Array::from(vec![1_234_567_890_123_i128, 9_876_543_210_987_i128])
        .with_precision_and_scale(38, 10)
        .unwrap();
    let columns: Vec<ArrayRef> = vec![
        Arc::new(BooleanArray::from(vec![true, false])),
        Arc::new(Int8Array::from(vec![10, 20])),
        Arc::new(Int16Array::from(vec![100, 200])),
        Arc::new(Int64Array::from(vec![0, 1])),
        Arc::new(Int64Array::from(vec![0, 1])),
        Arc::new(Int64Array::from(vec![2000, 2001])),
        Arc::new(Float32Array::from(vec![1.5, 2.5])),
        Arc::new(Float64Array::from(vec![3.25, 2.5])),
        Arc::new(decimal),
        Arc::new(Date32Array::from(vec![18706, 18707])),
        Arc::new(StringArray::from(vec!["test1", "test2"])),
        Arc::new(StringArray::from(vec!["queryID1", "queryID2"])),
        Arc::new(StringArray::from(vec!["region1", "region2"])),
    ];
    RecordBatch::try_new(Arc::new(schema), columns).unwrap()
}

fn checker(f: &Fixture) -> QueryStatusChecker {
    f.handler.context().query_status_checker(None, QUERY_ID)
}

fn id_name_result() -> ResultSetData {
    ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("id", JdbcType::BigInt).with_precision(19),
            ColumnMetadata::new("name", JdbcType::Varchar).with_precision(255),
        ],
        Vec::new(),
    )
}

fn export_request(split: Split) -> ReadRecordsRequest {
    let mut builder = SchemaBuilder::new_builder();
    builder.add_string_field("unused");
    request(
        builder.build(),
        split,
        Constraints::new(
            IndexMap::new(),
            Vec::new(),
            Vec::new(),
            -1,
            HashMap::from([
                (EXPORT_SCHEMA.to_string(), Some("true".to_string())),
                (QUERY.to_string(), Some(EXPORT_QUERY.to_string())),
            ]),
            None,
        ),
        100_000_000_000,
        100_000_000_000,
    )
}

fn put_keys(f: &Fixture) -> Vec<String> {
    f.fakes.s3.keys()
}

async fn assert_export_uses_jdbc(f: &Fixture, split: Split, reader: &FakeExportReader) {
    f.connection.on_query(|statement: &PreparedStatement| {
        assert_eq!(EXPORT_QUERY, statement.sql());
        Ok(ResultSetData::of_columns(
            vec![ColumnMetadata::new("id", JdbcType::BigInt).with_precision(19)],
            Vec::new(),
        ))
    });
    let response = f
        .handler
        .read_records(&f.allocator, &export_request(split))
        .await
        .unwrap();
    let FederationResponse::RemoteReadRecords(response) = response else {
        panic!("expected a RemoteReadRecordsResponse");
    };
    assert_eq!("id", response.schema().field(0).name());
    assert_eq!(vec![EXPORT_QUERY.to_string()], f.connection.prepared_sql());
    assert!(reader.reader_calls().is_empty());
    let sidecar = format!("spill-bucket/{}.schema", spill_location().key());
    assert!(put_keys(f).contains(&sidecar));
}

fn parquet_bytes(batch: &RecordBatch) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut bytes, batch.schema(), None).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
    bytes
}

#[tokio::test]
async fn do_read_records_no_spill() {
    let batch = root_batch();
    let f = fixture(
        true,
        Some(FakeExportReader::with_batches(vec![batch.clone()])),
    );
    let request = request(
        batch.schema().as_ref().clone(),
        export_split(Some("export_bucket"), Some("s3_object_key")).await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let response = f
        .handler
        .read_records(&f.allocator, &request)
        .await
        .unwrap();
    let FederationResponse::ReadRecords(response) = response else {
        panic!("expected a ReadRecordsResponse");
    };
    let records = response.records();
    assert_eq!(2, records.row_count());
    for (index, field) in batch.schema().fields().iter().enumerate() {
        let expected =
            FieldVector::from_array(Arc::clone(field), batch.column(index).as_ref()).unwrap();
        let actual = records.get_field_vector(field.name()).unwrap();
        assert_eq!(
            expected.get_object(0),
            actual.get_object(0),
            "{}",
            field.name()
        );
        assert_eq!(
            expected.get_object(1),
            actual.get_object(1),
            "{}",
            field.name()
        );
    }
}

#[tokio::test]
async fn do_read_records_spill() {
    let batch = root_batch();
    let f = fixture(
        true,
        Some(FakeExportReader::with_batches(vec![batch.clone()])),
    );
    let request = request(
        batch.schema().as_ref().clone(),
        export_split(Some("export_bucket"), Some("s3_object_key")).await,
        constraints(),
        1,
        0,
    );
    let response = f
        .handler
        .read_records(&f.allocator, &request)
        .await
        .unwrap();
    let FederationResponse::RemoteReadRecords(response) = response else {
        panic!("expected a RemoteReadRecordsResponse");
    };
    assert!(!response.remote_blocks().is_empty());
    let reader = S3BlockSpillReader::new(Arc::new(f.fakes.s3.clone()), Arc::clone(&f.allocator));
    let mut rows = 0;
    for location in response.remote_blocks() {
        let block = reader
            .read_with_schema(
                location.as_s3().unwrap(),
                response.encryption_key(),
                Arc::new(response.schema().clone()),
            )
            .await
            .unwrap();
        rows += block.row_count();
    }
    assert_eq!(2, rows);
}

#[tokio::test]
async fn test_handle_direct_read() {
    let f = fixture(false, None);
    f.connection
        .on_query(|_statement: &PreparedStatement| Ok(id_name_result()));
    let request = request(
        id_name_schema(),
        partition_split("partition-primary--limit-1000-offset-0").await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let response = f
        .handler
        .read_records(&f.allocator, &request)
        .await
        .unwrap();
    let FederationResponse::ReadRecords(response) = response else {
        panic!("expected a ReadRecordsResponse");
    };
    assert_eq!(0, response.record_count());
    assert_eq!(1, f.connection.executed().len());
}

#[tokio::test]
async fn test_build_split_sql() {
    let f = fixture(false, None);
    let statement = f
        .handler
        .build_split_sql(
            &f.connection,
            &SplitQuery {
                catalog_name: "testCatalog",
                table_name: &TableName::new("default", "test_table"),
                schema: &id_name_schema(),
                constraints: &constraints(),
                split: &partition_split("partition-primary-id-limit-1000-offset-0").await,
            },
        )
        .await
        .unwrap();
    assert_eq!(1, f.connection.prepared_sql().len());
    assert_eq!(
        "SELECT \"id\", \"name\" FROM \"default\".\"test_table\" ORDER BY id LIMIT 1000 OFFSET 0",
        statement.sql()
    );
    assert_eq!(1000, statement.fetch_size());
}

#[tokio::test]
async fn test_get_credential_provider() {
    let config = DatabaseConnectionConfig::new_with_secret(
        "testCatalog",
        snowflake_constants::SNOWFLAKE_NAME,
        "snowflake://jdbc:snowflake://hostname/",
        "testSecret",
    )
    .unwrap();
    let secret = config.secret().map(ToString::to_string);
    let f = fixture_with(config, false, None);
    let provider = f
        .handler
        .dialect()
        .credentials_provider(f.handler.context(), secret.as_deref(), None)
        .await
        .unwrap()
        .unwrap();
    assert!(provider.as_any().is::<SnowflakeCredentialsProvider>());
}

#[tokio::test]
async fn test_create_credentials_provider_passes_request_override_configuration() {
    let f = fixture(false, None);
    let override_config = AwsRequestOverrideConfiguration::builder().build();
    let provider = create_snowflake_credentials_provider(
        f.handler.context(),
        Some("testSecret"),
        Some(&override_config),
    )
    .unwrap();
    assert_eq!("testSecret", provider.oauth_secret_name());
    assert!(provider.request_override_configuration().is_some());
}

#[test]
fn test_convert_timestamp_tz_milli_to_date_milli_fast() {
    let ts = TimestampMillisecondArray::from(vec![
        1_609_459_200_000,
        1_609_545_600_000,
        1_609_632_000_000,
    ])
    .with_timezone("UTC");
    let result = convert_timestamp_tz_milli_to_date_milli_fast(&ts).unwrap();
    assert_eq!(3, result.len());
    assert_eq!(1_609_459_200_000, result.value(0));
    assert_eq!(1_609_545_600_000, result.value(1));
    assert_eq!(1_609_632_000_000, result.value(2));
}

#[test]
fn test_convert_timestamp_tz_milli_to_date_milli_fast_non_utc() {
    let ts =
        TimestampMillisecondArray::from(vec![1_609_459_200_000]).with_timezone("America/New_York");
    let error = convert_timestamp_tz_milli_to_date_milli_fast(&ts).unwrap_err();
    assert!(matches!(error, FederationError::IllegalArgument(_)));
    assert_eq!(
        "Athena S3 Export only support Timezone with UTC",
        error.to_string()
    );
}

#[tokio::test]
async fn test_handle_s3_export_read_empty_key() {
    let reader = FakeExportReader::default();
    let f = fixture(true, Some(reader.clone()));
    let request = request(
        id_name_schema(),
        export_split(Some("export_bucket"), Some("")).await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let response = f
        .handler
        .read_records(&f.allocator, &request)
        .await
        .unwrap();
    let FederationResponse::ReadRecords(response) = response else {
        panic!("expected a ReadRecordsResponse");
    };
    assert_eq!(0, response.record_count());
    assert!(reader.reader_calls().is_empty());
}

#[tokio::test]
async fn test_get_credential_provider_without_secret() {
    let config = DatabaseConnectionConfig::new(
        "testCatalog",
        snowflake_constants::SNOWFLAKE_NAME,
        "snowflake://jdbc:snowflake://hostname/",
    )
    .unwrap();
    let secret = config.secret().map(ToString::to_string);
    let f = fixture_with(config, false, None);
    assert!(f
        .handler
        .dialect()
        .credentials_provider(f.handler.context(), secret.as_deref(), None)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_build_split_sql_with_query_passthrough() {
    let f = fixture(false, None);
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        HashMap::from([
            (
                "schemaFunctionName".to_string(),
                Some("SYSTEM.QUERY".to_string()),
            ),
            (
                QUERY.to_string(),
                Some("SELECT * FROM custom_table".to_string()),
            ),
        ]),
        None,
    );
    let statement = f
        .handler
        .build_split_sql(
            &f.connection,
            &SplitQuery {
                catalog_name: "testCatalog",
                table_name: &TableName::new("default", "test_table"),
                schema: &id_name_schema(),
                constraints: &constraints,
                split: &partition_split("partition-primary--limit-1000-offset-0").await,
            },
        )
        .await
        .unwrap();
    assert_eq!("SELECT * FROM custom_table", statement.sql());
    assert_eq!(1000, statement.fetch_size());
}

#[tokio::test]
async fn test_read_with_constraint_s3_export() {
    let f = fixture(
        true,
        Some(FakeExportReader::with_batches(vec![root_batch()])),
    );
    let request = request(
        id_name_schema(),
        export_split(Some("export_bucket"), Some("test_key.parquet")).await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let spiller = CountingSpiller::default();
    f.handler
        .read_with_constraint(&spiller, &request, &checker(&f))
        .await
        .unwrap();
    assert!(spiller.write_rows_calls() >= 1);
}

#[tokio::test]
async fn test_read_with_constraint_direct_query() {
    let f = fixture(false, None);
    f.connection
        .on_query(|_statement: &PreparedStatement| Ok(id_name_result()));
    let request = request(
        id_name_schema(),
        partition_split("partition-primary--limit-1000-offset-0").await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let spiller = CountingSpiller::default();
    f.handler
        .read_with_constraint(&spiller, &request, &checker(&f))
        .await
        .unwrap();
    assert_eq!(1, f.connection.executed().len());
}

#[tokio::test]
async fn test_handle_s3_export_read_with_timestamp_tz() {
    let schema = Schema::new(vec![Field::new(
        "ts_col",
        DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
        true,
    )]);
    let batch = RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![Arc::new(
            TimestampMillisecondArray::from(vec![1_609_459_200_000]).with_timezone("UTC"),
        )],
    )
    .unwrap();
    let f = fixture(false, Some(FakeExportReader::with_batches(vec![batch])));
    let request = request(
        schema,
        export_split(Some("export_bucket"), Some("test_key.parquet")).await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let spiller = CountingSpiller::default();
    f.handler
        .dialect()
        .handle_s3_export_read(&spiller, &request, &checker(&f))
        .await
        .unwrap();
    assert!(spiller.write_rows_calls() >= 1);
}

#[tokio::test]
async fn test_handle_direct_read_method() {
    let f = fixture(false, None);
    f.connection
        .on_query(|_statement: &PreparedStatement| Ok(id_name_result()));
    let request = request(
        id_name_schema(),
        partition_split("partition-primary--limit-1000-offset-0").await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let spiller = CountingSpiller::default();
    f.handler
        .read_with_constraint(&spiller, &request, &checker(&f))
        .await
        .unwrap();
    assert_eq!(1, f.connection.prepared_sql().len());
}

#[test]
fn test_construct_s3_uri() {
    assert_eq!(
        "s3://test-bucket/test-key.parquet",
        construct_s3_uri("test-bucket", "test-key.parquet")
    );
}

#[tokio::test]
async fn test_build_split_sql_exception() {
    let f = fixture(false, None);
    f.connection
        .on_prepare(|_sql: &str| Err(SqlError::new("Test exception")));
    let error = f
        .handler
        .build_split_sql(
            &f.connection,
            &SplitQuery {
                catalog_name: "testCatalog",
                table_name: &TableName::new("default", "test_table"),
                schema: &id_name_schema(),
                constraints: &constraints(),
                split: &partition_split("partition-primary-id-limit-1000-offset-0").await,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException),
        error.error_code()
    );
    assert_eq!("Test exception", error.to_string());
}

#[test]
fn test_convert_timestamp_tz_milli_to_date_milli_fast_with_nulls() {
    let ts = TimestampMillisecondArray::from(vec![
        Some(1_609_459_200_000),
        None,
        Some(1_609_632_000_000),
    ])
    .with_timezone("UTC");
    let result = convert_timestamp_tz_milli_to_date_milli_fast(&ts).unwrap();
    assert_eq!(3, result.len());
    assert_eq!(1_609_459_200_000, result.value(0));
    assert!(arrow::array::Array::is_null(&result, 1));
    assert_eq!(1_609_632_000_000, result.value(2));
}

#[tokio::test]
async fn test_handle_s3_export_read_io_exception() {
    let f = fixture(true, Some(FakeExportReader::failing("Test IO exception")));
    let request = request(
        id_name_schema(),
        export_split(Some("export_bucket"), Some("test_key.parquet")).await,
        constraints(),
        100_000_000_000,
        100_000_000_000,
    );
    let spiller = CountingSpiller::default();
    let error = f
        .handler
        .dialect()
        .handle_s3_export_read(&spiller, &request, &checker(&f))
        .await
        .unwrap_err();
    assert!(error.as_connector_exception().is_some());
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException),
        error.error_code()
    );
    assert_eq!(
        "Error in object content for object : s3://export_bucket/test_key.parquet Test IO exception",
        error.to_string()
    );
}

#[tokio::test]
async fn test_construct_arrow_reader_with_projection() {
    let fakes = Fakes::new();
    let batch = root_batch();
    fakes.s3.objects.lock().unwrap().insert(
        "export_bucket/q/file.parquet".to_string(),
        parquet_bytes(&batch),
    );
    let reader = S3ParquetExportReader::new(Arc::new(fakes.s3.clone()));
    let uri = "s3://export_bucket/q/file.parquet";
    assert_eq!(
        batch.schema().as_ref(),
        &reader.inspect_parquet_schema(uri).await.unwrap()
    );
    let projection = Schema::new(vec![
        Field::new("queryId", DataType::Utf8, true),
        Field::new("day", DataType::Int64, true),
        Field::new("partition", DataType::Utf8, true),
    ]);
    let batches: Vec<RecordBatch> = reader
        .construct_arrow_reader(uri, &projection)
        .await
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(1, batches.len());
    let names: Vec<String> = batches[0]
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    assert_eq!(vec!["day".to_string(), "queryId".to_string()], names);
    assert_eq!(2, batches[0].num_rows());
}

#[tokio::test]
async fn export_schema_uses_jdbc_when_s3_export_disabled() {
    let reader = FakeExportReader::default();
    let f = fixture(false, Some(reader.clone()));
    let split = Split::new_builder(Some(spill_location().into()), Some(key().await)).build();
    assert_export_uses_jdbc(&f, split, &reader).await;
}

#[tokio::test]
async fn export_schema_uses_jdbc_when_s3_export_is_empty() {
    let reader = FakeExportReader::default();
    let f = fixture(true, Some(reader.clone()));
    let split = export_split(Some("export_bucket"), Some("")).await;
    assert_export_uses_jdbc(&f, split, &reader).await;
}

#[tokio::test]
async fn export_schema_respills_exported_parquet() {
    let batch = root_batch();
    let f = fixture(
        true,
        Some(FakeExportReader::with_batches(vec![batch.clone()])),
    );
    let split = export_split(Some("export_bucket"), Some("test_key.parquet")).await;
    let response = f
        .handler
        .read_records(&f.allocator, &export_request(split))
        .await
        .unwrap();
    let FederationResponse::RemoteReadRecords(response) = response else {
        panic!("expected a RemoteReadRecordsResponse");
    };
    assert_eq!(batch.schema().as_ref(), response.schema());
    assert_eq!(1, response.remote_blocks().len());
    let spill_key = spill_location().key().to_string();
    assert_eq!(
        format!("{spill_key}.0"),
        response.remote_blocks()[0].as_s3().unwrap().key()
    );
    assert!(put_keys(&f).contains(&format!("spill-bucket/{spill_key}.schema")));
    assert!(f.connection.prepared_sql().is_empty());
    let reader = S3BlockSpillReader::new(Arc::new(f.fakes.s3.clone()), Arc::clone(&f.allocator));
    let block = reader
        .read_with_schema(
            response.remote_blocks()[0].as_s3().unwrap(),
            response.encryption_key(),
            Arc::new(response.schema().clone()),
        )
        .await
        .unwrap();
    assert_eq!(2, block.row_count());
    let decimal = block
        .get_field_vector("decimalField")
        .unwrap()
        .get_object(1);
    assert_eq!(
        athena_federation_sdk::data::Value::Decimal(
            "987.6543210987".parse::<BigDecimal>().unwrap()
        ),
        decimal
    );
}

#[test]
fn derive_export_schema_maps_scale_zero_number_to_decimal() {
    let f = fixture(false, None);
    let metadata = ResultSetMetaData::new(vec![
        ColumnMetadata::new("number_19_0", JdbcType::BigInt)
            .with_precision(19)
            .with_scale(0),
        ColumnMetadata::new("number_precision_0", JdbcType::BigInt)
            .with_precision(0)
            .with_scale(0),
        ColumnMetadata::new("number_precision_39", JdbcType::BigInt)
            .with_precision(39)
            .with_scale(0),
        ColumnMetadata::new("bigint_scale_2", JdbcType::BigInt)
            .with_precision(19)
            .with_scale(2),
        ColumnMetadata::new("number_10_2", JdbcType::Decimal)
            .with_precision(10)
            .with_scale(2),
    ]);
    let schema = f
        .handler
        .dialect()
        .derive_export_schema(&f.handler, &metadata)
        .unwrap();
    let type_of = |name: &str| schema.field_with_name(name).unwrap().data_type().clone();
    assert_eq!(DataType::Decimal128(19, 0), type_of("number_19_0"));
    assert_eq!(DataType::Int64, type_of("number_precision_0"));
    assert_eq!(DataType::Int64, type_of("number_precision_39"));
    assert_eq!(DataType::Int64, type_of("bigint_scale_2"));
    assert_eq!(DataType::Decimal128(10, 2), type_of("number_10_2"));
}
