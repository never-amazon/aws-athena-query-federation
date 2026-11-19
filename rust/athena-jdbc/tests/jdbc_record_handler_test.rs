mod jdbc_support;

use athena_federation_sdk::data::date_time_formatter_util;
use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field};
use async_trait::async_trait;
use athena_federation_sdk::data::local_date_time_of_epoch_milli_utc;
use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, BlockSpiller, FieldBuilder, S3BlockSpiller, SchemaBuilder,
    Value,
};
use athena_federation_sdk::domain::predicate::{ConstraintEvaluator, Constraints};
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{HandlerContext, RecordHandler};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{BlockCrypto, EncryptionKey, FederatedIdentity, KeySource};
use athena_federation_sdk::serde::schema_from_bytes;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    self, make_field_writer_factory, ConnectionSource, JdbcRecordHandler, RecordDialect,
    SplitQuery, EXPORT_SCHEMA,
};
use athena_jdbc::qpt::QUERY;
use athena_jdbc::sql::{
    ColumnMetadata, JdbcConnection, JdbcType, PreparedStatement, ResultSetMetaData, Row, SqlValue,
};
use chrono::{NaiveDate, NaiveDateTime};
use indexmap::IndexMap;
use jdbc_support::{
    FakeConnection, FakeConnectionFactory, FakeSecretsManager, Fakes, ResultSetData,
};

const TEST_CATALOG: &str = "testCatalog";
const TEST_SECRET: &str = "testSecret";
const TEST_QUERY_ID: &str = "testQueryId";
const TEST_SCHEMA: &str = "testSchema";
const TEST_TABLE: &str = "testTable";
const TEST_COL1: &str = "testCol1";
const TEST_COL2: &str = "testCol2";
const TEST_PARTITION_COL: &str = "testPartitionCol";
const TEST_PARTITION_VALUE: &str = "testPartitionValue";
const TEST_VAL1: &str = "testVal1";
const TEST_VAL2: &str = "testVal2";
const CONNECTION_STRING: &str = "fakedatabase://jdbc:fakedatabase://hostname/${testSecret}";
const TEST_SQL: &str = "someSql";

struct TestDialect {
    export_only: bool,
}

#[async_trait]
impl RecordDialect for TestDialect {
    async fn build_split_sql(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        jdbc_connection: &dyn JdbcConnection,
        _query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        if self.export_only {
            return Err(FederationError::UnsupportedOperation(
                "export query must not call buildSplitSql".to_string(),
            ));
        }
        Ok(jdbc_connection.prepare_statement(TEST_SQL).await?)
    }
}

type TestJdbcRecordHandler = JdbcRecordHandler<TestDialect>;

struct Setup {
    fakes: Fakes,
    connection: FakeConnection,
    factory: FakeConnectionFactory,
    handler: TestJdbcRecordHandler,
}

fn config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new_with_secret(
        TEST_CATALOG,
        "fakedatabase",
        CONNECTION_STRING,
        TEST_SECRET,
    )
    .unwrap()
}

fn make_handler(
    fakes: &Fakes,
    factory: Option<FakeConnectionFactory>,
    export_only: bool,
) -> TestJdbcRecordHandler {
    let context = HandlerContext::new("fakedatabase", fakes.clients(), HashMap::new()).unwrap();
    let connection = match factory {
        Some(factory) => {
            let factory: Arc<dyn JdbcConnectionFactory> = Arc::new(factory);
            ConnectionSource::Direct {
                config: config(),
                factory,
            }
        }
        None => ConnectionSource::Delegating,
    };
    JdbcRecordHandler::new(context, connection, TestDialect { export_only })
}

fn set_up() -> Setup {
    let fakes = Fakes::with_secrets(FakeSecretsManager::with_secret(
        TEST_SECRET,
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ));
    let connection = FakeConnection::new();
    let factory = FakeConnectionFactory::new(connection.clone());
    let handler = make_handler(&fakes, Some(factory.clone()), false);
    Setup {
        fakes,
        connection,
        factory,
        handler,
    }
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

fn constraints_with(qpt_arguments: HashMap<String, Option<String>>) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        qpt_arguments,
        None,
    )
}

fn field(name: &str, data_type: DataType) -> Field {
    FieldBuilder::new_builder(name, data_type).build()
}

fn row(metadata: ResultSetMetaData, values: Vec<SqlValue>) -> Row {
    Row::new(Arc::new(metadata), values)
}

fn partition_map() -> HashMap<String, String> {
    HashMap::from([(
        TEST_PARTITION_COL.to_string(),
        TEST_PARTITION_VALUE.to_string(),
    )])
}

fn query_status_checker(handler: &TestJdbcRecordHandler) -> QueryStatusChecker {
    handler.context().query_status_checker(None, TEST_QUERY_ID)
}

#[tokio::test]
async fn close_delegates_to_jdbc_connection_factory() {
    let s = set_up();
    s.handler.close().await.unwrap();
    assert_eq!(1, s.factory.close_calls());
}

#[tokio::test]
async fn close_with_null_factory_is_no_op() {
    let s = set_up();
    let handler = make_handler(&s.fakes, None, false);
    handler.close().await.unwrap();
    assert_eq!(0, s.factory.close_calls());
}

#[tokio::test]
async fn read_with_constraint() {
    let s = set_up();
    let mut schema_builder = SchemaBuilder::new_builder();
    schema_builder.add_field(field(TEST_COL1, DataType::Int32));
    schema_builder.add_field(field(TEST_COL2, DataType::Utf8));
    schema_builder.add_field(field(TEST_PARTITION_COL, DataType::Utf8));
    let field_schema = schema_builder.build();

    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let spill_location = S3SpillLocation::new_builder()
        .with_bucket("bucket")
        .with_prefix("prefix")
        .with_query_id(TEST_QUERY_ID)
        .with_split_id("split")
        .build();
    let split = Split::new_builder(Some(spill_location.clone().into()), None)
        .add(TEST_PARTITION_COL, TEST_PARTITION_VALUE)
        .build();

    s.connection.meta.set_database_product_name("MySQL");
    s.connection.on_query(|_| {
        Ok(ResultSetData::of_columns(
            vec![
                ColumnMetadata::new(TEST_COL1, JdbcType::Integer),
                ColumnMetadata::new(TEST_COL2, JdbcType::Varchar),
            ],
            vec![
                vec![
                    SqlValue::Integer(1),
                    SqlValue::String(TEST_VAL1.to_string()),
                ],
                vec![
                    SqlValue::Integer(2),
                    SqlValue::String(TEST_VAL2.to_string()),
                ],
            ],
        ))
    });

    let spill_config = SpillConfig {
        max_block_bytes: 1,
        max_inline_block_bytes: 0,
        spill_threads: None,
        ..SpillConfig::new(spill_location.into(), TEST_QUERY_ID)
    };
    let spiller = S3BlockSpiller::new(
        Arc::new(s.fakes.s3.clone()),
        spill_config,
        Arc::clone(&allocator),
        Arc::new(field_schema.clone()),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();
    let request = ReadRecordsRequest::new(
        Some(identity()),
        TEST_CATALOG,
        TEST_QUERY_ID,
        TableName::new(TEST_SCHEMA, TEST_TABLE),
        field_schema,
        split,
        constraints_with(HashMap::new()),
        1024,
        1024,
    );
    let checker = query_status_checker(&s.handler);

    s.handler
        .read_with_constraint(&spiller, &request, &checker)
        .await
        .unwrap();
    spiller.close().await;
    drop(checker);

    let objects = s.fakes.s3.objects.lock().unwrap().clone();
    assert!(!objects.is_empty());
    for data in objects.values() {
        let text = String::from_utf8_lossy(data);
        assert!(
            text.contains(TEST_VAL1)
                || text.contains(TEST_VAL2)
                || text.contains(TEST_PARTITION_VALUE)
        );
    }
    assert_eq!(vec![TEST_SQL.to_string()], s.connection.prepared_sql());
    assert_eq!(vec![false], s.connection.auto_commit_calls());
    assert_eq!(1, s.connection.commits());
}

#[tokio::test]
async fn make_extractor() {
    let metadata = ResultSetMetaData::new(vec![
        ColumnMetadata::new(TEST_COL1, JdbcType::Integer),
        ColumnMetadata::new(TEST_COL2, JdbcType::Varchar),
        ColumnMetadata::new("testCol10", JdbcType::Double),
    ]);
    let partition_map = partition_map();
    let make = |name: &str, data_type: DataType| {
        manager::make_extractor(&field(name, data_type), &metadata, &partition_map).unwrap()
    };

    let actual_int = make(TEST_COL1, DataType::Int32);
    let actual_varchar = make(TEST_COL2, DataType::Utf8);
    let actual_bit = make("testCol3", DataType::Boolean);
    let actual_tiny_int = make("testCol4", DataType::Int8);
    let actual_small_int = make("testCol5", DataType::Int16);
    let actual_varbinary = make("testCol6", DataType::Binary);
    let actual_big_int = make("testCol8", DataType::Int64);
    let actual_float4 = make("testCol9", DataType::Float32);
    let actual_float8 = make("testCol10", DataType::Float64);
    let actual_date_day = make("testCol11", DataType::Date32);
    let actual_date_milli = make("testCol12", DataType::Date64);

    assert!(matches!(actual_int, Extractor::Int(_)));
    assert!(matches!(actual_varchar, Extractor::VarChar(_)));
    assert!(matches!(actual_bit, Extractor::Bit(_)));
    assert!(matches!(actual_tiny_int, Extractor::TinyInt(_)));
    assert!(matches!(actual_small_int, Extractor::SmallInt(_)));
    assert!(matches!(actual_varbinary, Extractor::VarBinary(_)));
    assert!(matches!(actual_big_int, Extractor::BigInt(_)));
    assert!(matches!(actual_float4, Extractor::Float4(_)));
    assert!(matches!(actual_float8, Extractor::Float8(_)));
    assert!(matches!(actual_date_day, Extractor::DateDay(_)));
    assert!(matches!(actual_date_milli, Extractor::DateMilli(_)));

    let context = row(
        metadata.clone(),
        vec![
            SqlValue::Integer(1),
            SqlValue::String(TEST_VAL1.to_string()),
            SqlValue::String("$1,000.50".to_string()),
        ],
    );
    let Extractor::Float8(float8) = actual_float8 else {
        panic!("expected a float8 extractor");
    };
    let dollar_value = float8(&context).unwrap().unwrap();
    assert!((dollar_value - 1000.5).abs() < f64::EPSILON);
}

fn extractor_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(vec![
        ColumnMetadata::new("testCol1", JdbcType::Integer),
        ColumnMetadata::new("testCol2", JdbcType::Varchar),
        ColumnMetadata::new("varchar_col", JdbcType::Varchar),
        ColumnMetadata::new("testCol3", JdbcType::Boolean),
        ColumnMetadata::new("testCol4", JdbcType::TinyInt),
        ColumnMetadata::new("testCol5", JdbcType::SmallInt),
        ColumnMetadata::new("testCol6", JdbcType::VarBinary),
        ColumnMetadata::new("testCol8", JdbcType::BigInt),
        ColumnMetadata::new("testCol9", JdbcType::Real),
        ColumnMetadata::new("testCol10", JdbcType::Double),
        ColumnMetadata::new("testCol11", JdbcType::Date),
        ColumnMetadata::new("testCol12", JdbcType::Timestamp),
    ])
}

struct Extractors {
    int: Extractor<Row>,
    varchar: Extractor<Row>,
    varchar_test: Extractor<Row>,
    bit: Extractor<Row>,
    tiny_int: Extractor<Row>,
    small_int: Extractor<Row>,
    varbinary: Extractor<Row>,
    big_int: Extractor<Row>,
    float4: Extractor<Row>,
    float8: Extractor<Row>,
    date_day: Extractor<Row>,
    date_milli: Extractor<Row>,
}

fn all_extractors(metadata: &ResultSetMetaData) -> Extractors {
    let partition_map = partition_map();
    let make = |name: &str, data_type: DataType| {
        manager::make_extractor(&field(name, data_type), metadata, &partition_map).unwrap()
    };
    Extractors {
        int: make("testCol1", DataType::Int32),
        varchar: make("testCol2", DataType::Utf8),
        varchar_test: make("varchar_col", DataType::Utf8),
        bit: make("testCol3", DataType::Boolean),
        tiny_int: make("testCol4", DataType::Int8),
        small_int: make("testCol5", DataType::Int16),
        varbinary: make("testCol6", DataType::Binary),
        big_int: make("testCol8", DataType::Int64),
        float4: make("testCol9", DataType::Float32),
        float8: make("testCol10", DataType::Float64),
        date_day: make("testCol11", DataType::Date32),
        date_milli: make("testCol12", DataType::Date64),
    }
}

fn int_value(e: &Extractor<Row>, context: &Row) -> Option<i32> {
    let Extractor::Int(e) = e else { panic!("int") };
    e(context).unwrap()
}

fn varchar_value(e: &Extractor<Row>, context: &Row) -> Option<String> {
    let Extractor::VarChar(e) = e else {
        panic!("varchar")
    };
    e(context).unwrap()
}

fn bit_value(e: &Extractor<Row>, context: &Row) -> Option<i32> {
    let Extractor::Bit(e) = e else { panic!("bit") };
    e(context).unwrap()
}

fn tiny_int_value(e: &Extractor<Row>, context: &Row) -> Option<i8> {
    let Extractor::TinyInt(e) = e else {
        panic!("tinyint")
    };
    e(context).unwrap()
}

fn small_int_value(e: &Extractor<Row>, context: &Row) -> Option<i16> {
    let Extractor::SmallInt(e) = e else {
        panic!("smallint")
    };
    e(context).unwrap()
}

fn varbinary_value(e: &Extractor<Row>, context: &Row) -> Option<Vec<u8>> {
    let Extractor::VarBinary(e) = e else {
        panic!("varbinary")
    };
    e(context).unwrap()
}

fn big_int_value(e: &Extractor<Row>, context: &Row) -> Option<i64> {
    let Extractor::BigInt(e) = e else {
        panic!("bigint")
    };
    e(context).unwrap()
}

fn float4_value(e: &Extractor<Row>, context: &Row) -> Option<f32> {
    let Extractor::Float4(e) = e else {
        panic!("float4")
    };
    e(context).unwrap()
}

fn float8_value(e: &Extractor<Row>, context: &Row) -> Option<f64> {
    let Extractor::Float8(e) = e else {
        panic!("float8")
    };
    e(context).unwrap()
}

fn date_day_value(e: &Extractor<Row>, context: &Row) -> Option<i32> {
    let Extractor::DateDay(e) = e else {
        panic!("dateday")
    };
    e(context).unwrap()
}

fn date_milli_value(e: &Extractor<Row>, context: &Row) -> Option<i64> {
    let Extractor::DateMilli(e) = e else {
        panic!("datemilli")
    };
    e(context).unwrap()
}

#[tokio::test]
async fn test_make_extractor_without_null_values() {
    let metadata = extractor_metadata();
    let bytes = b"test".to_vec();
    let date = NaiveDate::from_ymd_opt(2025, 4, 22).unwrap();
    let time = NaiveDateTime::parse_from_str("2025-04-22 05:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let context = row(
        metadata.clone(),
        vec![
            SqlValue::Integer(10),
            SqlValue::String("test".to_string()),
            SqlValue::String("test value".to_string()),
            SqlValue::Boolean(true),
            SqlValue::TinyInt(100),
            SqlValue::SmallInt(1234),
            SqlValue::Bytes(bytes.clone()),
            SqlValue::BigInt(10000),
            SqlValue::Real(123.0),
            SqlValue::Double(1.5),
            SqlValue::Date(date),
            SqlValue::Timestamp(time),
        ],
    );
    let e = all_extractors(&metadata);

    assert_eq!(Some(10), int_value(&e.int, &context));
    assert_eq!(
        Some("test".to_string()),
        varchar_value(&e.varchar, &context)
    );
    assert_eq!(
        Some("test value".to_string()),
        varchar_value(&e.varchar_test, &context)
    );
    assert_eq!(Some(1), bit_value(&e.bit, &context));
    assert_eq!(Some(100), tiny_int_value(&e.tiny_int, &context));
    assert_eq!(Some(1234), small_int_value(&e.small_int, &context));
    assert_eq!(Some(bytes), varbinary_value(&e.varbinary, &context));
    assert_eq!(Some(10000), big_int_value(&e.big_int, &context));
    let float4 = float4_value(&e.float4, &context).unwrap();
    assert!((float4 - 123.0).abs() < f32::EPSILON);
    assert_eq!(Some(20200), date_day_value(&e.date_day, &context));
    assert_eq!(
        Some(time.and_utc().timestamp_millis()),
        date_milli_value(&e.date_milli, &context)
    );
}

#[tokio::test]
async fn make_extractor_reads_date_as_timestamp_by_calendar_date() {
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("testCol12", JdbcType::Date)]);
    let extractor = manager::make_extractor(
        &field("testCol12", DataType::Date64),
        &metadata,
        &partition_map(),
    )
    .unwrap();
    for (year, month, day) in [(1000, 1, 1), (1582, 10, 4), (1, 1, 1), (2025, 4, 22)] {
        let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
        let context = row(metadata.clone(), vec![SqlValue::Date(date)]);
        let epoch_day = date.signed_duration_since(NaiveDate::default()).num_days();
        assert_eq!(
            Some(epoch_day * 86_400_000),
            date_milli_value(&extractor, &context),
            "{date}"
        );
    }
}

#[tokio::test]
async fn make_extractor_reads_dates_from_year_ten_thousand() {
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("testCol", JdbcType::Date)]);
    let extractor = |data_type| {
        manager::make_extractor(&field("testCol", data_type), &metadata, &partition_map()).unwrap()
    };
    let (day_extractor, milli_extractor) =
        (extractor(DataType::Date32), extractor(DataType::Date64));
    for (year, month, day, expected) in [
        (9999, 12, 31, 2_932_896_i64),
        (10000, 1, 1, 2_932_897),
        (123_456, 7, 8, 44_372_039),
        (5_874_897, 12, 31, 2_145_042_905),
    ] {
        let label = format!("+{year}-{month:02}-{day:02}");
        let context = row(
            metadata.clone(),
            vec![SqlValue::java_sql_date(
                expected * 86_400_000,
                expected,
                label.clone(),
            )],
        );
        assert_eq!(
            Some(expected),
            date_day_value(&day_extractor, &context).map(i64::from),
            "{label}"
        );
        assert_eq!(
            Some(expected * 86_400_000),
            date_milli_value(&milli_extractor, &context),
            "{label}"
        );
    }
}

fn write_array(element: DataType, values: Vec<SqlValue>) -> Value {
    let mut schema_builder = SchemaBuilder::new_builder();
    schema_builder.add_list_field("values", element);
    let schema = Arc::new(schema_builder.build());
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("values", JdbcType::Array)]);
    let context = row(metadata, vec![SqlValue::Array(values)]);
    let mut block = BlockAllocatorImpl::new().create_block(Arc::clone(&schema));
    let vector = block.get_field_vector_mut("values").unwrap();
    let mut writer = make_field_writer_factory(schema.field(0))(vector, None, None).unwrap();
    writer.write(vector, &context, 0).unwrap();
    block.set_row_count(1);
    block.get_field_vector("values").unwrap().get_object(0)
}

#[tokio::test]
async fn make_factory_reads_pre1582_array_dates_by_calendar_date() {
    let dates = [
        ((1000, 1, 1), -354_280),
        ((1582, 10, 4), -141_428),
        ((1582, 10, 15), -141_427),
        ((2025, 4, 22), 20_200),
    ];
    let elements = || {
        let mut elements: Vec<SqlValue> = dates
            .iter()
            .map(|&((year, month, day), java_day)| {
                let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
                SqlValue::java_sql_date(java_day * 86_400_000, epoch_day(date), date.to_string())
            })
            .collect();
        elements.insert(2, SqlValue::Null);
        elements
    };
    let epoch_days = || {
        let mut days: Vec<i64> = dates
            .iter()
            .map(|&((year, month, day), _)| {
                NaiveDate::from_ymd_opt(year, month, day)
                    .unwrap()
                    .signed_duration_since(NaiveDate::default())
                    .num_days()
            })
            .collect::<Vec<_>>();
        days.insert(2, i64::MIN);
        days
    };
    let as_value =
        |day: i64, f: &dyn Fn(i64) -> Value| if day == i64::MIN { Value::Null } else { f(day) };
    let expected_days = Value::List(
        epoch_days()
            .into_iter()
            .map(|day| as_value(day, &|day| Value::Int(i32::try_from(day).unwrap())))
            .collect(),
    );
    assert_eq!(expected_days, write_array(DataType::Date32, elements()));
    let expected_millis = Value::List(
        epoch_days()
            .into_iter()
            .map(|day| {
                as_value(day, &|day| {
                    Value::LocalDateTime(
                        NaiveDate::default()
                            .checked_add_signed(chrono::TimeDelta::days(day))
                            .unwrap()
                            .and_hms_opt(0, 0, 0)
                            .unwrap(),
                    )
                })
            })
            .collect(),
    );
    assert_eq!(expected_millis, write_array(DataType::Date64, elements()));
}

#[tokio::test]
async fn make_factory_reads_array_dates_like_scalar_dates() {
    let dates = || {
        vec![
            SqlValue::java_sql_date(9_223_372_036_825_200_000, 106_751_991_166, "infinity"),
            SqlValue::java_sql_date(-9_223_372_036_832_400_000, -106_749_799_148, "-infinity"),
            SqlValue::java_sql_date(253_402_300_800_000, 2_932_897, "+10000-01-01"),
            SqlValue::java_sql_date(1_745_280_000_000, 20_200, "2025-04-22"),
        ]
    };
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("testCol", JdbcType::Date)]);
    let extractor = |data_type| {
        manager::make_extractor(&field("testCol", data_type), &metadata, &partition_map()).unwrap()
    };
    let (day_extractor, milli_extractor) =
        (extractor(DataType::Date32), extractor(DataType::Date64));
    let scalar = |date: SqlValue| row(metadata.clone(), vec![date]);
    let expected_days = Value::List(
        dates()
            .into_iter()
            .map(|date| Value::Int(date_day_value(&day_extractor, &scalar(date)).unwrap()))
            .collect(),
    );
    assert_eq!(expected_days, write_array(DataType::Date32, dates()));
    let expected_millis = Value::List(
        dates()
            .into_iter()
            .map(|date| {
                Value::LocalDateTime(local_date_time_of_epoch_milli_utc(
                    date_milli_value(&milli_extractor, &scalar(date)).unwrap(),
                ))
            })
            .collect(),
    );
    assert_eq!(expected_millis, write_array(DataType::Date64, dates()));
}

#[tokio::test]
async fn make_factory_reads_array_timestamps_under_date_day_by_calendar_date() {
    let timestamp = |label: NaiveDateTime, java: NaiveDateTime| {
        SqlValue::java_timestamp(
            SqlValue::Timestamp(java),
            label.to_string(),
            label.and_utc().timestamp_millis(),
            epoch_day(label.date()),
        )
    };
    let at = |y, m, d, h, mi, sec, ms| {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_milli_opt(h, mi, sec, ms)
            .unwrap()
    };
    let elements = vec![
        timestamp(at(1000, 1, 1, 12, 0, 0, 0), at(1000, 1, 6, 12, 0, 0, 0)),
        timestamp(
            at(1582, 10, 4, 23, 59, 59, 999),
            at(1582, 10, 14, 23, 59, 59, 999),
        ),
        SqlValue::Null,
        SqlValue::java_date_millis(9_223_372_036_825_200_000, 106_751_991_166, "infinity"),
        SqlValue::java_date_millis(-9_223_372_036_832_400_000, -106_749_799_148, "-infinity"),
        timestamp(at(2020, 6, 1, 12, 0, 0, 123), at(2020, 6, 1, 12, 0, 0, 123)),
    ];
    let day = |y, m, d| {
        Value::Int(i32::try_from(epoch_day(NaiveDate::from_ymd_opt(y, m, d).unwrap())).unwrap())
    };
    assert_eq!(
        Value::List(vec![
            day(1000, 1, 1),
            day(1582, 10, 4),
            Value::Null,
            Value::Int(i32::MAX),
            Value::Int(i32::MIN),
            day(2020, 6, 1),
        ]),
        write_array(DataType::Date32, elements)
    );
}

#[tokio::test]
async fn make_factory_reads_array_timestamps_through_get_timestamp_millis() {
    let timestamps = [
        NaiveDate::from_ymd_opt(1000, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        NaiveDate::from_ymd_opt(-43, 3, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        NaiveDate::from_ymd_opt(2020, 6, 1)
            .unwrap()
            .and_hms_milli_opt(12, 0, 0, 123)
            .unwrap(),
    ];
    let driver_timestamp = |timestamp: NaiveDateTime, label: NaiveDateTime, text: &str| {
        SqlValue::java_timestamp(
            SqlValue::Timestamp(label),
            text,
            timestamp.and_utc().timestamp_millis(),
            epoch_day(label.date()),
        )
    };
    let elements = vec![
        driver_timestamp(
            timestamps[0],
            timestamps[0] + chrono::TimeDelta::days(5),
            "1000-01-01 12:00:00",
        ),
        driver_timestamp(
            timestamps[1],
            timestamps[1] - chrono::TimeDelta::days(2),
            "0044-03-15 12:00:00 BC",
        ),
        SqlValue::Null,
        SqlValue::Timestamp(timestamps[2]),
    ];
    assert_eq!(
        Value::List(vec![
            Value::LocalDateTime(timestamps[0]),
            Value::LocalDateTime(timestamps[1]),
            Value::Null,
            Value::LocalDateTime(timestamps[2]),
        ]),
        write_array(DataType::Date64, elements)
    );
}

#[tokio::test]
async fn test_make_extractor_with_null_values() {
    let metadata = extractor_metadata();
    let context = row(metadata.clone(), vec![SqlValue::Null; 12]);
    let e = all_extractors(&metadata);

    assert_eq!(None, int_value(&e.int, &context));
    assert_eq!(None, varchar_value(&e.varchar, &context));
    assert_eq!(None, varchar_value(&e.varchar_test, &context));
    assert_eq!(None, bit_value(&e.bit, &context));
    assert_eq!(None, tiny_int_value(&e.tiny_int, &context));
    assert_eq!(None, small_int_value(&e.small_int, &context));
    assert_eq!(None, varbinary_value(&e.varbinary, &context));
    assert_eq!(None, big_int_value(&e.big_int, &context));
    assert_eq!(None, float4_value(&e.float4, &context));
    assert_eq!(None, float8_value(&e.float8, &context));
    assert_eq!(None, date_day_value(&e.date_day, &context));
    assert_eq!(None, date_milli_value(&e.date_milli, &context));
}

#[tokio::test]
async fn test_make_extractor_with_unsupported_type() {
    let unsupported_field = field(
        "testColUnsupported",
        DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
    );
    let Err(e) = manager::make_extractor(
        &unsupported_field,
        &ResultSetMetaData::new(Vec::new()),
        &partition_map(),
    ) else {
        panic!("Expected AthenaConnectorException for unsupported type LIST");
    };
    let connector = e.as_connector_exception().unwrap();
    assert_eq!("Unhandled type LIST", connector.message);
    assert_eq!(
        FederationSourceErrorCode::OperationNotSupportedException,
        connector.code
    );
}

#[tokio::test]
async fn test_build_query_passthrough_sql() {
    let s = set_up();
    let test_query = "SELECT * FROM test_table";
    let constraints = constraints_with(HashMap::from([
        ("QUERY".to_string(), Some(test_query.to_string())),
        (
            "schemaFunctionName".to_string(),
            Some("SYSTEM.QUERY".to_string()),
        ),
    ]));
    let result = s
        .handler
        .build_query_passthrough_sql(&s.connection, &constraints)
        .await
        .unwrap();
    assert_eq!(test_query, result.sql());
    assert_eq!(vec![test_query.to_string()], s.connection.prepared_sql());
}

#[tokio::test]
async fn test_build_query_passthrough_sql_with_exception() {
    let s = set_up();
    let constraints = constraints_with(HashMap::from([(
        "QUERY".to_string(),
        Some("SELECT * FROM test_table".to_string()),
    )]));
    let e = s
        .handler
        .build_query_passthrough_sql(&s.connection, &constraints)
        .await
        .unwrap_err();
    assert!(e.as_connector_exception().is_some());
    assert!(e
        .to_string()
        .contains("Function Signature doesn't match implementation's"));
}

#[tokio::test]
async fn test_make_extractor_float8_with_sql_exception() {
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new(TEST_COL1, JdbcType::Double)]);
    let extractor = manager::make_extractor(
        &field(TEST_COL1, DataType::Float64),
        &metadata,
        &HashMap::new(),
    )
    .unwrap();
    let context = row(metadata, vec![SqlValue::String("$25,000.50".to_string())]);
    let value = float8_value(&extractor, &context).unwrap();
    assert!((value - 25000.50).abs() < 0.01);
}

#[tokio::test]
async fn test_make_extractor_with_partition_values() {
    let metadata = ResultSetMetaData::new(Vec::new());
    let extractor = manager::make_extractor(
        &field(TEST_PARTITION_COL, DataType::Utf8),
        &metadata,
        &partition_map(),
    )
    .unwrap();
    assert_eq!(
        Some(TEST_PARTITION_VALUE.to_string()),
        varchar_value(&extractor, &row(metadata, Vec::new()))
    );
}

#[tokio::test]
async fn test_enable_case_sensitively_look_up_session() {
    let s = set_up();
    let result = s
        .handler
        .dialect()
        .enable_case_sensitively_look_up_session(&FakeConnection::new())
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_disable_case_sensitively_look_up_session() {
    let s = set_up();
    let result = s
        .handler
        .dialect()
        .disable_case_sensitively_look_up_session(&FakeConnection::new())
        .await
        .unwrap();
    assert!(!result);
}

fn export_spill_location() -> S3SpillLocation {
    S3SpillLocation::new_builder()
        .with_bucket("test-bucket")
        .with_prefix("prefix")
        .with_query_id("q1")
        .with_split_id("split1")
        .build()
}

struct ExportRun {
    fakes: Fakes,
    response: athena_federation_sdk::records::RemoteReadRecordsResponse,
}

async fn run_export(
    key: &EncryptionKey,
    data: ResultSetData,
    query: &str,
    max_block_bytes: i64,
) -> ExportRun {
    let fakes = Fakes::with_secrets(FakeSecretsManager::with_secret(
        TEST_SECRET,
        "{\"username\": \"testUser\", \"password\": \"testPassword\"}",
    ));
    let connection = FakeConnection::new();
    let expected_query = query.to_string();
    connection.on_query(move |statement| {
        assert_eq!(expected_query, statement.sql());
        Ok(data.clone())
    });
    let handler = make_handler(&fakes, Some(FakeConnectionFactory::new(connection)), true);

    let split = Split::new_builder(Some(export_spill_location().into()), Some(key.clone())).build();
    let mut schema_builder = SchemaBuilder::new_builder();
    schema_builder.add_field(field("name", DataType::Utf8));
    let request = ReadRecordsRequest::new(
        Some(identity()),
        TEST_CATALOG,
        TEST_QUERY_ID,
        TableName::new(TEST_SCHEMA, TEST_TABLE),
        schema_builder.build(),
        split,
        constraints_with(HashMap::from([
            (EXPORT_SCHEMA.to_string(), Some("true".to_string())),
            (QUERY.to_string(), Some(query.to_string())),
        ])),
        max_block_bytes,
        100_000_000_000,
    );
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let response = handler.read_records(&allocator, &request).await.unwrap();
    let FederationResponse::RemoteReadRecords(response) = response else {
        panic!("expected a RemoteReadRecordsResponse, got {response:?}");
    };
    ExportRun { fakes, response }
}

fn object(fakes: &Fakes, key: &str) -> Option<Vec<u8>> {
    fakes
        .s3
        .objects
        .lock()
        .unwrap()
        .get(&format!("test-bucket/{key}"))
        .cloned()
}

fn crypto(key: &EncryptionKey) -> BlockCrypto {
    BlockCrypto::AesGcm(key.clone())
}

#[tokio::test]
async fn test_do_read_records_direct_query_uses_spiller_and_returns_locations() {
    let key = KeySource::Local.create(None).await.unwrap();
    let orders = NaiveDateTime::parse_from_str("2021-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let customers =
        NaiveDateTime::parse_from_str("2021-02-02 12:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let data = ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("CREATED_ON", JdbcType::Timestamp),
            ColumnMetadata::new("NAME", JdbcType::Varchar),
        ],
        vec![
            vec![
                SqlValue::Timestamp(customers),
                SqlValue::String("customers_view".to_string()),
            ],
            vec![
                SqlValue::Timestamp(orders),
                SqlValue::String("orders_view".to_string()),
            ],
        ],
    );
    let run = run_export(
        &key,
        data,
        "SELECT created_on, name FROM v ORDER BY name",
        100_000_000_000,
    )
    .await;

    let spill_key = export_spill_location().key().to_string();
    let remote = &run.response;
    assert_eq!(1, remote.remote_blocks().len());
    assert_eq!(
        format!("{spill_key}.0"),
        remote.remote_blocks()[0].as_s3().unwrap().key()
    );
    assert_eq!(2, remote.schema().fields().len());
    assert!(object(&run.fakes, &format!("{spill_key}.0")).is_some());
    let sidecar = object(&run.fakes, &format!("{spill_key}.schema")).unwrap();

    let crypto = crypto(&key);
    let schema = schema_from_bytes(&crypto.decrypt(&sidecar).unwrap()).unwrap();
    assert_eq!(2, schema.fields().len());
    let block = crypto
        .decrypt_with_schema(
            &BlockAllocatorImpl::new(),
            &object(&run.fakes, &format!("{spill_key}.0")).unwrap(),
            Arc::new(schema),
        )
        .unwrap();
    assert_eq!(2, block.row_count());
    let names = block.get_field_vector("NAME").unwrap();
    let created_on = block.get_field_vector("CREATED_ON").unwrap();
    let names: Vec<Value> = (0..2).map(|i| names.get_object(i)).collect();
    let created_on: Vec<Value> = (0..2).map(|i| created_on.get_raw(i)).collect();
    assert!(names.contains(&Value::String("orders_view".to_string())));
    assert!(names.contains(&Value::String("customers_view".to_string())));

    let orders_created_on = orders.and_utc().timestamp_millis();
    assert!(created_on.contains(&Value::Long(orders_created_on)));
    assert!(created_on.contains(&Value::Long(customers.and_utc().timestamp_millis())));
    assert_ne!(
        orders_created_on,
        date_time_formatter_util::timestamp_milli_tz_holder_from_object(
            &Value::Date(orders_created_on),
            Some("UTC"),
            date_time_formatter_util::TzEncoding::Packed
        )
        .unwrap()
        .value
    );
}

#[tokio::test]
async fn test_do_read_records_direct_query_keeps_duplicate_column_labels() {
    let key = KeySource::Local.create(None).await.unwrap();
    let data = ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("ID", JdbcType::Integer),
            ColumnMetadata::new("ID", JdbcType::Integer),
        ],
        vec![vec![SqlValue::Integer(1), SqlValue::Integer(2)]],
    );
    let run = run_export(
        &key,
        data,
        "SELECT a.id, b.id FROM a CROSS JOIN b",
        100_000_000_000,
    )
    .await;

    let data_location = run.response.remote_blocks()[0].as_s3().unwrap();
    let block = crypto(&key)
        .decrypt_with_schema(
            &BlockAllocatorImpl::new(),
            &object(&run.fakes, data_location.key()).unwrap(),
            Arc::new(run.response.schema().clone()),
        )
        .unwrap();
    assert_eq!(Value::Int(1), block.field_vectors()[0].get_object(0));
    assert_eq!(Value::Int(2), block.field_vectors()[1].get_object(0));
}

#[tokio::test]
async fn export_reads_pre1582_values_on_the_proleptic_calendar() {
    let key = KeySource::Local.create(None).await.unwrap();
    let timestamps = [
        NaiveDate::from_ymd_opt(1000, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        NaiveDate::from_ymd_opt(1582, 10, 4)
            .unwrap()
            .and_hms_milli_opt(23, 59, 59, 999)
            .unwrap(),
        NaiveDate::from_ymd_opt(2020, 6, 1)
            .unwrap()
            .and_hms_milli_opt(12, 0, 0, 123)
            .unwrap(),
    ];
    let java_days = [-354_280, -141_428, 18_414];
    let timestamp = |i: usize| {
        let ts: NaiveDateTime = timestamps[i];
        SqlValue::java_timestamp(
            SqlValue::Timestamp(ts + chrono::TimeDelta::days(java_days[i] - epoch_day(ts.date()))),
            ts.to_string(),
            ts.and_utc().timestamp_millis(),
            java_days[i],
        )
    };
    let java_date = |i: usize| {
        let label = timestamps[i].date();
        SqlValue::java_sql_date(
            java_days[i] * 86_400_000,
            epoch_day(label),
            label.to_string(),
        )
    };
    let data = ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("D", JdbcType::Date),
            ColumnMetadata::new("TS", JdbcType::Timestamp),
            ColumnMetadata::new("TS", JdbcType::Timestamp),
        ],
        (0..2)
            .map(|i| vec![java_date(i), timestamp(i), timestamp(i + 1)])
            .collect(),
    );
    let run = run_export(
        &key,
        data,
        "SELECT a.d, a.ts, b.ts FROM c a JOIN c b ON b.id = a.id + 1 ORDER BY a.id",
        100_000_000_000,
    )
    .await;
    let data_location = run.response.remote_blocks()[0].as_s3().unwrap();
    let block = crypto(&key)
        .decrypt_with_schema(
            &BlockAllocatorImpl::new(),
            &object(&run.fakes, data_location.key()).unwrap(),
            Arc::new(run.response.schema().clone()),
        )
        .unwrap();
    assert_eq!(2, block.row_count());
    for row in 0..2 {
        assert_eq!(
            Value::Int(i32::try_from(epoch_day(timestamps[row].date())).unwrap()),
            block.field_vectors()[0].get_object(row)
        );
        assert_eq!(
            Value::Long(timestamps[row].and_utc().timestamp_millis()),
            block.field_vectors()[1].get_raw(row)
        );
        assert_eq!(
            Value::Long(timestamps[row + 1].and_utc().timestamp_millis()),
            block.field_vectors()[2].get_raw(row)
        );
    }
}

fn epoch_day(date: NaiveDate) -> i64 {
    date.signed_duration_since(NaiveDate::default()).num_days()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_do_read_records_direct_query_returns_every_block_with_async_spill() {
    let key = KeySource::Local.create(None).await.unwrap();
    let row_count = 200;
    let data = ResultSetData::of_columns(
        vec![ColumnMetadata::new("ID", JdbcType::Integer)],
        (1..=row_count)
            .map(|i| vec![SqlValue::Integer(i)])
            .collect(),
    );
    let run = run_export(&key, data, "SELECT id FROM n", 1).await;

    let spill_key = export_spill_location().key().to_string();
    let expected_keys: std::collections::HashSet<String> =
        (0..row_count).map(|i| format!("{spill_key}.{i}")).collect();
    let crypto = crypto(&key);
    let mut returned_keys = std::collections::HashSet::new();
    let mut returned_rows = 0;
    for location in run.response.remote_blocks() {
        let location_key = location.as_s3().unwrap().key().to_string();
        returned_rows += crypto
            .decrypt_with_schema(
                &BlockAllocatorImpl::new(),
                &object(&run.fakes, &location_key).unwrap(),
                Arc::new(run.response.schema().clone()),
            )
            .unwrap()
            .row_count();
        returned_keys.insert(location_key);
    }
    assert_eq!(
        usize::try_from(row_count).unwrap(),
        run.response.remote_blocks().len()
    );
    assert_eq!(expected_keys, returned_keys);
    assert_eq!(usize::try_from(row_count).unwrap(), returned_rows);
}
