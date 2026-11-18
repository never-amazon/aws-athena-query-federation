mod jdbc_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::writers::holders::{
    NullableBigIntHolder, NullableBitHolder, NullableDateDayHolder, NullableDateMilliHolder,
    NullableFloat4Holder, NullableFloat8Holder, NullableIntHolder, NullableSmallIntHolder,
    NullableTinyIntHolder, NullableVarBinaryHolder, NullableVarCharHolder,
};
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, BlockSpiller, DateTimeFormatterUtil, FieldBuilder,
    S3BlockSpiller, SchemaBuilder, Value,
};
use athena_federation_sdk::domain::predicate::{ConstraintEvaluator, Constraints};
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{FederationRequestHandler, RecordHandler, RecordHandlerBase};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::security::{
    AesGcmBlockCrypto, BlockCrypto, CachableSecretsManager, EncryptionKey, EncryptionKeyFactory,
    FederatedIdentity, KmsEncryptionProvider, LocalKeyFactory,
};
use athena_federation_sdk::serde::schema_from_bytes;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{
    JdbcRecordHandler, JdbcRecordHandlerBase, JdbcRecordHandlerExt, EXPORT_SCHEMA,
};
use athena_jdbc::qpt::QUERY;
use athena_jdbc::sql::{
    ColumnMetadata, JdbcConnection, PreparedStatement, ResultSetMetaData, Row, SqlValue, Types,
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

struct TestJdbcRecordHandler {
    base: JdbcRecordHandlerBase,
    export_only: bool,
}

impl FederationRequestHandler for TestJdbcRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.record_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.record_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl RecordHandler for TestJdbcRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        self.base.record_base()
    }

    async fn do_read_records(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        self.jdbc_do_read_records(allocator, request).await
    }

    async fn read_with_constraint(
        &self,
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.jdbc_read_with_constraint(spiller, request, query_status_checker)
            .await
    }
}

#[async_trait]
impl JdbcRecordHandler for TestJdbcRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn build_split_sql(
        &self,
        jdbc_connection: &dyn JdbcConnection,
        _catalog_name: &str,
        _table_name: &TableName,
        _schema: &Schema,
        _constraints: &Constraints,
        _split: &Split,
    ) -> Result<PreparedStatement> {
        if self.export_only {
            return Err(FederationError::UnsupportedOperation(
                "export query must not call buildSplitSql".to_string(),
            ));
        }
        Ok(jdbc_connection.prepare_statement(TEST_SQL).await?)
    }
}

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
    let record_base =
        RecordHandlerBase::new("fakedatabase", fakes.clients(), HashMap::new()).unwrap();
    let base = match factory {
        Some(factory) => {
            let factory: Arc<dyn JdbcConnectionFactory> = Arc::new(factory);
            JdbcRecordHandlerBase::new(record_base, config(), factory)
        }
        None => JdbcRecordHandlerBase::new_without_connection(record_base),
    };
    TestJdbcRecordHandler { base, export_only }
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

fn query_status_checker(fakes: &Fakes, handler: &TestJdbcRecordHandler) -> QueryStatusChecker {
    QueryStatusChecker::new(
        fakes.athena.clone(),
        Arc::clone(handler.record_base().athena_invoker()),
        TEST_QUERY_ID,
    )
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
        .with_is_directory(true)
        .build();
    let split = Split::new_builder(Some(spill_location.clone().into()), None)
        .add(TEST_PARTITION_COL, TEST_PARTITION_VALUE)
        .build();

    s.connection.meta.set_database_product_name("MySQL");
    s.connection.on_query(|_| {
        Ok(ResultSetData::of_columns(
            vec![
                ColumnMetadata::new(TEST_COL1, Types::INTEGER),
                ColumnMetadata::new(TEST_COL2, Types::VARCHAR),
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

    let spill_config = SpillConfig::new_builder()
        .with_spill_location(spill_location.into())
        .with_request_id(TEST_QUERY_ID)
        .with_max_block_bytes(1)
        .with_max_inline_block_bytes(0)
        .with_num_spill_threads(0)
        .build()
        .unwrap();
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
    let checker = query_status_checker(&s.fakes, &s.handler);

    s.handler
        .read_with_constraint(&spiller, &request, &checker)
        .await
        .unwrap();
    spiller.close().await;
    checker.close();

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
    let s = set_up();
    let metadata = ResultSetMetaData::new(vec![
        ColumnMetadata::new(TEST_COL1, Types::INTEGER),
        ColumnMetadata::new(TEST_COL2, Types::VARCHAR),
        ColumnMetadata::new("testCol10", Types::DOUBLE),
    ]);
    let partition_map = partition_map();
    let make = |name: &str, data_type: DataType| {
        s.handler
            .make_extractor(&field(name, data_type), &metadata, &partition_map)
            .unwrap()
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
    let mut dollar_value = NullableFloat8Holder::default();
    float8.extract(&context, &mut dollar_value).unwrap();
    assert!((dollar_value.value - 1000.5).abs() < f64::EPSILON);
}

fn extractor_metadata() -> ResultSetMetaData {
    ResultSetMetaData::new(vec![
        ColumnMetadata::new("testCol1", Types::INTEGER),
        ColumnMetadata::new("testCol2", Types::VARCHAR),
        ColumnMetadata::new("varchar_col", Types::VARCHAR),
        ColumnMetadata::new("testCol3", Types::BOOLEAN),
        ColumnMetadata::new("testCol4", Types::TINYINT),
        ColumnMetadata::new("testCol5", Types::SMALLINT),
        ColumnMetadata::new("testCol6", Types::VARBINARY),
        ColumnMetadata::new("testCol8", Types::BIGINT),
        ColumnMetadata::new("testCol9", Types::REAL),
        ColumnMetadata::new("testCol10", Types::DOUBLE),
        ColumnMetadata::new("testCol11", Types::DATE),
        ColumnMetadata::new("testCol12", Types::TIMESTAMP),
    ])
}

struct Extractors {
    int: Extractor,
    varchar: Extractor,
    varchar_test: Extractor,
    bit: Extractor,
    tiny_int: Extractor,
    small_int: Extractor,
    varbinary: Extractor,
    big_int: Extractor,
    float4: Extractor,
    float8: Extractor,
    date_day: Extractor,
    date_milli: Extractor,
}

fn all_extractors(handler: &TestJdbcRecordHandler, metadata: &ResultSetMetaData) -> Extractors {
    let partition_map = partition_map();
    let make = |name: &str, data_type: DataType| {
        handler
            .make_extractor(&field(name, data_type), metadata, &partition_map)
            .unwrap()
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

fn int_holder(e: &Extractor, context: &Row) -> NullableIntHolder {
    let Extractor::Int(e) = e else { panic!("int") };
    let mut h = NullableIntHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn varchar_holder(e: &Extractor, context: &Row) -> NullableVarCharHolder {
    let Extractor::VarChar(e) = e else {
        panic!("varchar")
    };
    let mut h = NullableVarCharHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn bit_holder(e: &Extractor, context: &Row) -> NullableBitHolder {
    let Extractor::Bit(e) = e else { panic!("bit") };
    let mut h = NullableBitHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn tiny_int_holder(e: &Extractor, context: &Row) -> NullableTinyIntHolder {
    let Extractor::TinyInt(e) = e else {
        panic!("tinyint")
    };
    let mut h = NullableTinyIntHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn small_int_holder(e: &Extractor, context: &Row) -> NullableSmallIntHolder {
    let Extractor::SmallInt(e) = e else {
        panic!("smallint")
    };
    let mut h = NullableSmallIntHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn varbinary_holder(e: &Extractor, context: &Row) -> NullableVarBinaryHolder {
    let Extractor::VarBinary(e) = e else {
        panic!("varbinary")
    };
    let mut h = NullableVarBinaryHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn big_int_holder(e: &Extractor, context: &Row) -> NullableBigIntHolder {
    let Extractor::BigInt(e) = e else {
        panic!("bigint")
    };
    let mut h = NullableBigIntHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn float4_holder(e: &Extractor, context: &Row) -> NullableFloat4Holder {
    let Extractor::Float4(e) = e else {
        panic!("float4")
    };
    let mut h = NullableFloat4Holder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn float8_holder(e: &Extractor, context: &Row) -> NullableFloat8Holder {
    let Extractor::Float8(e) = e else {
        panic!("float8")
    };
    let mut h = NullableFloat8Holder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn date_day_holder(e: &Extractor, context: &Row) -> NullableDateDayHolder {
    let Extractor::DateDay(e) = e else {
        panic!("dateday")
    };
    let mut h = NullableDateDayHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

fn date_milli_holder(e: &Extractor, context: &Row) -> NullableDateMilliHolder {
    let Extractor::DateMilli(e) = e else {
        panic!("datemilli")
    };
    let mut h = NullableDateMilliHolder::default();
    e.extract(context, &mut h).unwrap();
    h
}

#[tokio::test]
async fn test_make_extractor_without_null_values() {
    let s = set_up();
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
    let e = all_extractors(&s.handler, &metadata);

    let h = int_holder(&e.int, &context);
    assert_eq!((10, 1), (h.value, h.is_set));
    let h = varchar_holder(&e.varchar, &context);
    assert_eq!(("test".to_string(), 1), (h.value, h.is_set));
    let h = varchar_holder(&e.varchar_test, &context);
    assert_eq!(("test value".to_string(), 1), (h.value, h.is_set));
    let h = bit_holder(&e.bit, &context);
    assert_eq!((1, 1), (h.value, h.is_set));
    let h = tiny_int_holder(&e.tiny_int, &context);
    assert_eq!((100, 1), (h.value, h.is_set));
    let h = small_int_holder(&e.small_int, &context);
    assert_eq!((1234, 1), (h.value, h.is_set));
    let h = varbinary_holder(&e.varbinary, &context);
    assert_eq!((bytes, 1), (h.value, h.is_set));
    let h = big_int_holder(&e.big_int, &context);
    assert_eq!((10000, 1), (h.value, h.is_set));
    let h = float4_holder(&e.float4, &context);
    assert!((h.value - 123.0).abs() < f32::EPSILON);
    assert_eq!(1, h.is_set);
    let h = date_day_holder(&e.date_day, &context);
    assert_eq!(1, h.is_set);
    assert_eq!(20200, h.value);
    let h = date_milli_holder(&e.date_milli, &context);
    assert_eq!(1, h.is_set);
    assert_eq!(time.and_utc().timestamp_millis(), h.value);
}

#[tokio::test]
async fn make_extractor_reads_date_as_timestamp_by_calendar_date() {
    let s = set_up();
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("testCol12", Types::DATE)]);
    let extractor = s
        .handler
        .make_extractor(
            &field("testCol12", DataType::Date64),
            &metadata,
            &partition_map(),
        )
        .unwrap();
    for (year, month, day) in [(1000, 1, 1), (1582, 10, 4), (1, 1, 1), (2025, 4, 22)] {
        let date = NaiveDate::from_ymd_opt(year, month, day).unwrap();
        let context = row(metadata.clone(), vec![SqlValue::Date(date)]);
        let h = date_milli_holder(&extractor, &context);
        let epoch_day = date.signed_duration_since(NaiveDate::default()).num_days();
        assert_eq!((epoch_day * 86_400_000, 1), (h.value, h.is_set), "{date}");
    }
}

fn write_array(handler: &TestJdbcRecordHandler, element: DataType, values: Vec<SqlValue>) -> Value {
    let mut schema_builder = SchemaBuilder::new_builder();
    schema_builder.add_list_field("values", element);
    let schema = Arc::new(schema_builder.build());
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new("values", Types::ARRAY)]);
    let context = row(metadata, vec![SqlValue::Array(values)]);
    let mut block = BlockAllocatorImpl::new().create_block(Arc::clone(&schema));
    let vector = block.get_field_vector_mut("values").unwrap();
    let mut writer = handler
        .make_factory(schema.field(0))
        .create(vector, None, None)
        .unwrap();
    writer.write(vector, &context, 0).unwrap();
    block.set_row_count(1);
    block.get_field_vector("values").unwrap().get_object(0)
}

#[tokio::test]
async fn make_factory_reads_pre1582_array_dates_by_calendar_date() {
    let s = set_up();
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
                SqlValue::java_sql_date(java_day * 86_400_000, date, date.to_string())
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
    assert_eq!(
        expected_days,
        write_array(&s.handler, DataType::Date32, elements())
    );
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
    assert_eq!(
        expected_millis,
        write_array(&s.handler, DataType::Date64, elements())
    );
}

#[tokio::test]
async fn make_factory_reads_array_timestamps_through_get_timestamp_millis() {
    let s = set_up();
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
    let elements = vec![
        SqlValue::rendered_with_epoch_millis(
            SqlValue::Timestamp(timestamps[0] + chrono::TimeDelta::days(5)),
            "1000-01-01 12:00:00",
            timestamps[0].and_utc().timestamp_millis(),
        ),
        SqlValue::rendered_with_epoch_millis(
            SqlValue::Timestamp(timestamps[1] - chrono::TimeDelta::days(2)),
            "0044-03-15 12:00:00 BC",
            timestamps[1].and_utc().timestamp_millis(),
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
        write_array(&s.handler, DataType::Date64, elements)
    );
}

#[tokio::test]
async fn test_make_extractor_with_null_values() {
    let s = set_up();
    let metadata = extractor_metadata();
    let context = row(metadata.clone(), vec![SqlValue::Null; 12]);
    let e = all_extractors(&s.handler, &metadata);

    let h = int_holder(&e.int, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = varchar_holder(&e.varchar, &context);
    assert_eq!(0, h.is_set);
    assert!(h.value.is_empty());
    let h = varchar_holder(&e.varchar_test, &context);
    assert_eq!(0, h.is_set);
    assert!(h.value.is_empty());
    let h = bit_holder(&e.bit, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = tiny_int_holder(&e.tiny_int, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = small_int_holder(&e.small_int, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = varbinary_holder(&e.varbinary, &context);
    assert_eq!(0, h.is_set);
    assert!(h.value.is_empty());
    let h = big_int_holder(&e.big_int, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = float4_holder(&e.float4, &context);
    assert_eq!(0, h.is_set);
    assert!(h.value.abs() < f32::EPSILON);
    let h = float8_holder(&e.float8, &context);
    assert_eq!(0, h.is_set);
    assert!(h.value.abs() < f64::EPSILON);
    let h = date_day_holder(&e.date_day, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
    let h = date_milli_holder(&e.date_milli, &context);
    assert_eq!((0, 0), (h.is_set, h.value));
}

#[tokio::test]
async fn test_make_extractor_with_unsupported_type() {
    let s = set_up();
    let unsupported_field = field(
        "testColUnsupported",
        DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
    );
    let Err(e) = s.handler.make_extractor(
        &unsupported_field,
        &ResultSetMetaData::new(Vec::new()),
        &partition_map(),
    ) else {
        panic!("Expected AthenaConnectorException for unsupported type LIST");
    };
    let connector = e.as_connector_exception().unwrap();
    assert_eq!("Unhandled type LIST", connector.message());
    assert_eq!(
        Some(FederationSourceErrorCode::OperationNotSupportedException.as_str()),
        connector.error_details().error_code()
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
    let s = set_up();
    let metadata = ResultSetMetaData::new(vec![ColumnMetadata::new(TEST_COL1, Types::DOUBLE)]);
    let extractor = s
        .handler
        .make_extractor(
            &field(TEST_COL1, DataType::Float64),
            &metadata,
            &HashMap::new(),
        )
        .unwrap();
    let context = row(metadata, vec![SqlValue::String("$25,000.50".to_string())]);
    let h = float8_holder(&extractor, &context);
    assert_eq!(1, h.is_set);
    assert!((h.value - 25000.50).abs() < 0.01);
}

#[tokio::test]
async fn test_make_extractor_with_partition_values() {
    let s = set_up();
    let metadata = ResultSetMetaData::new(Vec::new());
    let extractor = s
        .handler
        .make_extractor(
            &field(TEST_PARTITION_COL, DataType::Utf8),
            &metadata,
            &partition_map(),
        )
        .unwrap();
    let h = varchar_holder(&extractor, &row(metadata, Vec::new()));
    assert_eq!(1, h.is_set);
    assert_eq!(TEST_PARTITION_VALUE, h.value);
}

#[tokio::test]
async fn test_enable_case_sensitively_look_up_session() {
    let s = set_up();
    let result = s
        .handler
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
        .with_is_directory(true)
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
    let response = handler.do_read_records(&allocator, &request).await.unwrap();
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

fn crypto() -> AesGcmBlockCrypto {
    AesGcmBlockCrypto::new(Arc::new(BlockAllocatorImpl::new()))
}

#[tokio::test]
async fn test_do_read_records_direct_query_uses_spiller_and_returns_locations() {
    let key = LocalKeyFactory::new().create().await.unwrap();
    let orders = NaiveDateTime::parse_from_str("2021-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let customers =
        NaiveDateTime::parse_from_str("2021-02-02 12:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
    let data = ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("CREATED_ON", Types::TIMESTAMP),
            ColumnMetadata::new("NAME", Types::VARCHAR),
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

    let crypto = crypto();
    let schema = schema_from_bytes(&crypto.decrypt(Some(&key), &sidecar).unwrap()).unwrap();
    assert_eq!(2, schema.fields().len());
    let block = crypto
        .decrypt_with_schema(
            Some(&key),
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
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(
            &Value::Date(orders_created_on),
            Some("UTC")
        )
        .unwrap()
        .value
    );
}

#[tokio::test]
async fn test_do_read_records_direct_query_keeps_duplicate_column_labels() {
    let key = LocalKeyFactory::new().create().await.unwrap();
    let data = ResultSetData::of_columns(
        vec![
            ColumnMetadata::new("ID", Types::INTEGER),
            ColumnMetadata::new("ID", Types::INTEGER),
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
    let block = crypto()
        .decrypt_with_schema(
            Some(&key),
            &object(&run.fakes, data_location.key()).unwrap(),
            Arc::new(run.response.schema().clone()),
        )
        .unwrap();
    assert_eq!(Value::Int(1), block.field_vectors()[0].get_object(0));
    assert_eq!(Value::Int(2), block.field_vectors()[1].get_object(0));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_do_read_records_direct_query_returns_every_block_with_async_spill() {
    let key = LocalKeyFactory::new().create().await.unwrap();
    let row_count = 200;
    let data = ResultSetData::of_columns(
        vec![ColumnMetadata::new("ID", Types::INTEGER)],
        (1..=row_count)
            .map(|i| vec![SqlValue::Integer(i)])
            .collect(),
    );
    let run = run_export(&key, data, "SELECT id FROM n", 1).await;

    let spill_key = export_spill_location().key().to_string();
    let expected_keys: std::collections::HashSet<String> =
        (0..row_count).map(|i| format!("{spill_key}.{i}")).collect();
    let crypto = crypto();
    let mut returned_keys = std::collections::HashSet::new();
    let mut returned_rows = 0;
    for location in run.response.remote_blocks() {
        let location_key = location.as_s3().unwrap().key().to_string();
        returned_rows += crypto
            .decrypt_with_schema(
                Some(&key),
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
