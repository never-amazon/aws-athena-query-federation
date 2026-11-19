#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{
    Constraints, Direction, Marker, OrderByField, QueryPlan, Range, SortedRangeSet, ValueSet,
    DEFAULT_NO_LIMIT,
};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::HandlerContext;
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{RecordDialect, SplitQuery};
use athena_jdbc::sql::{
    ColumnMetadata, JdbcType, PreparedStatement, ResultSetMetaData, SqlError, SqlValue,
};
use athena_postgresql::{constants, PostGreSqlRecordDialect, PostGreSqlRecordHandler};
use bigdecimal::BigDecimal;
use chrono::NaiveDate;
use indexmap::IndexMap;
use jdbc_support::{FakeConnection, FakeConnectionFactory, Fakes, ResultSetData};

const TEST_SCHEMA: &str = "testSchema";
const TEST_TABLE: &str = "testTable";
const TEST_CATALOG: &str = "testCatalogName";
const PARTITION_SCHEMA: &str = "s0";
const PARTITION_NAME: &str = "p0";
const PARTITION_SCHEMA_NAME: &str = "partition_schema_name";
const PARTITION_NAME_COL: &str = "partition_name";
const CHAR_COLUMNS_QUERY: &str = "SELECT column_name FROM information_schema.columns WHERE table_schema = ? AND table_name = ? AND data_type = 'character'";
const SUBSTRAIT_PLAN_FETCH: &str = "GlsSWQpXGlUKAgoAEksKSQoCCgASKAoEY29sMQoEY29sMgoEY29sMxIUCgRiAhABCgQqAhABCgRiAhABGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUYACAK";

struct Fixture {
    handler: PostGreSqlRecordHandler,
    connection: FakeConnection,
    allocator: BlockAllocatorImpl,
}

fn setup() -> Fixture {
    let fakes = Fakes::new();
    let connection = FakeConnection::new();
    connection.on_query(|statement| {
        if statement.sql() == CHAR_COLUMNS_QUERY {
            Ok(ResultSetData::of_strings(
                &["column_name"],
                &[&["testCol10"]],
            ))
        } else {
            Ok(ResultSetData::empty(&[]))
        }
    });
    let factory: Arc<dyn JdbcConnectionFactory> =
        Arc::new(FakeConnectionFactory::new(connection.clone()));
    let config = DatabaseConnectionConfig::new(
        "testCatalog",
        constants::POSTGRES_NAME,
        "postgres://jdbc:postgresql://hostname/user=A&password=B",
    )
    .unwrap();
    let context = HandlerContext::new(config.engine(), fakes.clients(), HashMap::new()).unwrap();
    let handler = PostGreSqlRecordDialect::handler(context, config, factory);
    Fixture {
        handler,
        connection,
        allocator: BlockAllocatorImpl::new(),
    }
}

impl Fixture {
    async fn build(
        &self,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> athena_federation_sdk::Result<PreparedStatement> {
        self.handler
            .build_split_sql(
                &self.connection,
                &SplitQuery {
                    catalog_name: TEST_CATALOG,
                    table_name,
                    schema,
                    constraints,
                    split,
                },
            )
            .await
    }

    fn last_prepared(&self) -> String {
        self.connection
            .prepared_sql()
            .into_iter()
            .rfind(|sql| sql != CHAR_COLUMNS_QUERY)
            .unwrap()
    }

    fn single(&self, data_type: &DataType, value: impl Into<Value>) -> ValueSet {
        ValueSet::from(
            SortedRangeSet::of_ranges(
                Range::equal(&self.allocator, data_type, value).unwrap(),
                Vec::new(),
            )
            .unwrap(),
        )
    }

    fn multi(&self, data_type: &DataType, values: Vec<Value>) -> ValueSet {
        let ranges: Vec<Range> = values
            .into_iter()
            .map(|v| Range::equal(&self.allocator, data_type, v).unwrap())
            .collect();
        ValueSet::from(SortedRangeSet::copy_of(data_type, ranges, false).unwrap())
    }

    fn range(
        &self,
        data_type: &DataType,
        low_inclusive: bool,
        low: impl Into<Value>,
        high_inclusive: bool,
        high: impl Into<Value>,
    ) -> ValueSet {
        let a = &self.allocator;
        let low = if low_inclusive {
            Marker::exactly(a, data_type, low).unwrap()
        } else {
            Marker::above(a, data_type, low).unwrap()
        };
        let high = if high_inclusive {
            Marker::exactly(a, data_type, high).unwrap()
        } else {
            Marker::below(a, data_type, high).unwrap()
        };
        ValueSet::from(
            SortedRangeSet::of_ranges(Range::new(low, high).unwrap(), Vec::new()).unwrap(),
        )
    }
}

fn table() -> TableName {
    TableName::new(TEST_SCHEMA, TEST_TABLE)
}

fn split(partition_schema: &str, partition_name: &str) -> Split {
    Split::new(
        None,
        None,
        HashMap::from([
            (
                PARTITION_SCHEMA_NAME.to_string(),
                partition_schema.to_string(),
            ),
            (PARTITION_NAME_COL.to_string(), partition_name.to_string()),
        ]),
    )
}

fn schema_with(fields: &[(&str, DataType)]) -> Schema {
    let mut all = vec![
        Field::new(PARTITION_SCHEMA_NAME, DataType::Utf8, true),
        Field::new(PARTITION_NAME_COL, DataType::Utf8, true),
    ];
    all.extend(fields.iter().map(|(n, t)| Field::new(*n, t.clone(), true)));
    Schema::new(all)
}

fn constraints_full(
    summary: Vec<(&str, ValueSet)>,
    order_by: Vec<OrderByField>,
    limit: i64,
) -> Constraints {
    let summary: IndexMap<String, ValueSet> = summary
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    Constraints::new(summary, Vec::new(), order_by, limit, HashMap::new(), None)
}

fn constraints(summary: Vec<(&str, ValueSet)>) -> Constraints {
    constraints_full(summary, Vec::new(), DEFAULT_NO_LIMIT)
}

fn dec(text: &str) -> BigDecimal {
    BigDecimal::from_str(text).unwrap()
}

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn days(text: &str) -> i32 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
    i32::try_from((date(text) - epoch).num_days()).unwrap()
}

fn s(text: &str) -> SqlValue {
    SqlValue::String(text.to_string())
}

async fn assert_build(
    f: &Fixture,
    schema: &Schema,
    constraints: &Constraints,
    expected_sql: &str,
    expected_parameters: &[SqlValue],
) {
    let statement = f
        .build(
            &table(),
            schema,
            constraints,
            &split(PARTITION_SCHEMA, PARTITION_NAME),
        )
        .await
        .unwrap();
    assert_eq!(expected_sql, statement.sql());
    assert_eq!(expected_sql, f.last_prepared());
    assert_eq!(expected_parameters, statement.parameters());
}

#[tokio::test]
async fn build_split_sql_test() {
    let f = setup();
    let schema = schema_with(&[
        ("testCol1", DataType::Int32),
        ("testCol2", DataType::Utf8),
        ("testCol3", DataType::Int64),
        ("testCol4", DataType::Float32),
        ("testCol5", DataType::Int16),
        ("testCol6", DataType::Int8),
        ("testCol7", DataType::Float64),
        ("testCol8", DataType::Boolean),
        ("testCol9", DataType::Decimal128(8, 2)),
        ("testCol10", DataType::Utf8),
    ]);
    let c = constraints(vec![
        (
            "testCol1",
            f.multi(&DataType::Int32, vec![Value::Int(1), Value::Int(2)]),
        ),
        ("testCol2", f.range(&DataType::Utf8, true, "1", false, "10")),
        (
            "testCol3",
            f.range(&DataType::Int64, false, 2_i64, true, 20_i64),
        ),
        ("testCol4", f.single(&DataType::Float32, 1.1_f32)),
        ("testCol5", f.single(&DataType::Int16, 1_i16)),
        ("testCol6", f.single(&DataType::Int8, 0_i8)),
        ("testCol7", f.single(&DataType::Float64, 1.2_f64)),
        ("testCol8", f.single(&DataType::Boolean, true)),
        (
            "testCol9",
            f.single(&DataType::Decimal128(8, 2), dec("12.34")),
        ),
        ("testCol10", f.single(&DataType::Utf8, "A")),
    ]);
    let expected = "SELECT \"testCol1\", \"testCol2\", \"testCol3\", \"testCol4\", \"testCol5\", \"testCol6\", \"testCol7\", \"testCol8\", \"testCol9\", RTRIM(\"testCol10\") AS \"testCol10\" FROM \"s0\".\"p0\"  WHERE (\"testCol1\" IN (?,?)) AND ((\"testCol2\" >= ? AND \"testCol2\" < ?)) AND ((\"testCol3\" > ? AND \"testCol3\" <= ?)) AND (\"testCol4\" = ?) AND (\"testCol5\" = ?) AND (\"testCol6\" = ?) AND (\"testCol7\" = ?) AND (\"testCol8\" = ?) AND (\"testCol9\" = ?) AND (\"testCol10\" = ?)";
    assert_build(
        &f,
        &schema,
        &c,
        expected,
        &[
            SqlValue::Integer(1),
            SqlValue::Integer(2),
            s("1"),
            s("10"),
            SqlValue::BigInt(2),
            SqlValue::BigInt(20),
            SqlValue::Real(1.1),
            SqlValue::SmallInt(1),
            SqlValue::TinyInt(0),
            SqlValue::Double(1.2),
            SqlValue::Boolean(true),
            SqlValue::Decimal(dec("12.34")),
            s("A"),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_for_date_test() {
    let f = setup();
    let schema = schema_with(&[("testDate", DataType::Date32)]);
    let c = constraints(vec![(
        "testDate",
        f.single(&DataType::Date32, days("2020-01-05")),
    )]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"testDate\" FROM \"s0\".\"p0\"  WHERE (\"testDate\" = ?)",
        &[SqlValue::Date(date("2020-01-05"))],
    )
    .await;
}

#[tokio::test]
async fn build_sql_binds_pre1582_timestamps_and_dates_as_java_time() {
    let f = setup();
    let schema = schema_with(&[("ts", DataType::Date64), ("d", DataType::Date32)]);
    let timestamp = NaiveDate::from_ymd_opt(1000, 1, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let date = NaiveDate::from_ymd_opt(-43, 3, 15).unwrap();
    let epoch_day =
        i32::try_from(date.signed_duration_since(NaiveDate::default()).num_days()).unwrap();
    let c = constraints(vec![
        (
            "ts",
            f.single(&DataType::Date64, Value::LocalDateTime(timestamp)),
        ),
        ("d", f.single(&DataType::Date32, epoch_day)),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"ts\", \"d\" FROM \"s0\".\"p0\"  WHERE (\"ts\" = ?) AND (\"d\" = ?)",
        &[SqlValue::Timestamp(timestamp), SqlValue::Date(date)],
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_rejects_timestamps_outside_the_postgre_sql_range() {
    let f = setup();
    let schema = schema_with(&[("ts", DataType::Date64)]);
    let timestamp = NaiveDate::from_ymd_opt(-5000, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let c = constraints(vec![(
        "ts",
        f.single(&DataType::Date64, Value::LocalDateTime(timestamp)),
    )]);
    let error = f
        .build(
            &table(),
            &schema,
            &c,
            &split(PARTITION_SCHEMA, PARTITION_NAME),
        )
        .await
        .unwrap_err();
    assert_eq!(
        "Timestamp is outside the PostgreSQL range 4714-11-24 00:00:00 BC to 294276-12-31 23:59:59.999999",
        error.as_connector_exception().unwrap().message
    );
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException),
        error.error_code()
    );
}

async fn assert_prepare_error(value: ValueSet, f: &Fixture, message: &'static str) {
    f.connection.on_prepare(move |sql| {
        if sql == CHAR_COLUMNS_QUERY {
            Ok(PreparedStatement::new(sql))
        } else {
            Err(SqlError::new(message))
        }
    });
    let schema = schema_with(&[("col1", DataType::Int32)]);
    let error = f
        .build(
            &table(),
            &schema,
            &constraints(vec![("col1", value)]),
            &split(PARTITION_SCHEMA, PARTITION_NAME),
        )
        .await
        .unwrap_err();
    let connector = error.as_connector_exception().unwrap();
    assert_eq!(message, connector.message);
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException),
        error.error_code()
    );
}

#[tokio::test]
async fn build_split_sql_with_connection_error_test() {
    let f = setup();
    let value = f.single(&DataType::Int32, 1);
    assert_prepare_error(value, &f, "Connection refused").await;
}

#[tokio::test]
async fn build_split_sql_with_syntax_error_test() {
    let f = setup();
    let value = f.single(&DataType::Utf8, "invalid'value");
    assert_prepare_error(value, &f, "Syntax error in SQL statement").await;
}

#[tokio::test]
async fn build_split_sql_with_permission_error_test() {
    let f = setup();
    let value = f.single(&DataType::Int32, 1);
    assert_prepare_error(value, &f, "permission denied for table testTable").await;
}

#[tokio::test]
async fn build_split_sql_with_empty_result_test() {
    let f = setup();
    let schema = schema_with(&[("col1", DataType::Int32), ("col2", DataType::Utf8)]);
    let c = constraints(vec![
        ("col1", f.range(&DataType::Int32, false, 5, false, 10)),
        ("col2", f.single(&DataType::Utf8, "test")),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"col1\", \"col2\" FROM \"s0\".\"p0\"  WHERE ((\"col1\" > ? AND \"col1\" < ?)) AND (\"col2\" = ?)",
        &[SqlValue::Integer(5), SqlValue::Integer(10), s("test")],
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_with_large_numbers_test() {
    let f = setup();
    let schema = Schema::new(vec![
        Field::new("bigintCol", DataType::Int64, true),
        Field::new("decimal38_10", DataType::Decimal128(38, 10), true),
        Field::new("decimal18_6", DataType::Decimal128(18, 6), true),
        Field::new("decimal9_3", DataType::Decimal128(9, 3), true),
        Field::new(PARTITION_SCHEMA_NAME, DataType::Utf8, true),
        Field::new(PARTITION_NAME_COL, DataType::Utf8, true),
    ]);
    let c = constraints(vec![
        ("bigintCol", f.single(&DataType::Int64, i64::MAX)),
        (
            "decimal38_10",
            f.single(
                &DataType::Decimal128(38, 10),
                dec("12345678901234567890.1234567890"),
            ),
        ),
        (
            "decimal18_6",
            f.single(&DataType::Decimal128(18, 6), dec("123456789012.123456")),
        ),
        (
            "decimal9_3",
            f.single(&DataType::Decimal128(9, 3), dec("123456.789")),
        ),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"bigintCol\", \"decimal38_10\", \"decimal18_6\", \"decimal9_3\" FROM \"s0\".\"p0\"  WHERE (\"bigintCol\" = ?) AND (\"decimal38_10\" = ?) AND (\"decimal18_6\" = ?) AND (\"decimal9_3\" = ?)",
        &[
            SqlValue::BigInt(i64::MAX),
            SqlValue::Decimal(dec("12345678901234567890.1234567890")),
            SqlValue::Decimal(dec("123456789012.123456")),
            SqlValue::Decimal(dec("123456.789")),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_with_special_chars_test() {
    let f = setup();
    let names = [
        "column with spaces",
        "column\"with\"quotes",
        "column'with'apostrophes",
        "column;with;semicolons",
    ];
    let values = [
        "value with spaces",
        "value\"with\"quotes",
        "value'with'apostrophes",
        "value;with;semicolons",
    ];
    let schema = schema_with(&names.map(|n| (n, DataType::Utf8)));
    let c = constraints(
        names
            .iter()
            .zip(values)
            .map(|(n, v)| (*n, f.single(&DataType::Utf8, v)))
            .collect(),
    );
    let statement = f
        .build(
            &TableName::new("test Schema", "test Table"),
            &schema,
            &c,
            &split(PARTITION_SCHEMA, PARTITION_NAME),
        )
        .await
        .unwrap();
    let expected = "SELECT \"column with spaces\", \"column\"\"with\"\"quotes\", \"column'with'apostrophes\", \"column;with;semicolons\" FROM \"s0\".\"p0\"  WHERE (\"column with spaces\" = ?) AND (\"column\"\"with\"\"quotes\" = ?) AND (\"column'with'apostrophes\" = ?) AND (\"column;with;semicolons\" = ?)";
    assert_eq!(expected, statement.sql());
    assert_eq!(values.map(s).to_vec(), statement.parameters());
}

#[tokio::test]
async fn build_split_sql_with_json_test() {
    let f = setup();
    let json = "{\"key\": \"value\", \"number\": 42}";
    let jsonb = "{\"array\": [1, 2, 3], \"nested\": {\"field\": \"test\"}}";
    let schema = schema_with(&[("jsonCol", DataType::Utf8), ("jsonbCol", DataType::Utf8)]);
    let c = constraints(vec![
        ("jsonCol", f.single(&DataType::Utf8, json)),
        ("jsonbCol", f.single(&DataType::Utf8, jsonb)),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"jsonCol\", \"jsonbCol\" FROM \"s0\".\"p0\"  WHERE (\"jsonCol\" = ?) AND (\"jsonbCol\" = ?)",
        &[s(json), s(jsonb)],
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_with_array_test() {
    let f = setup();
    let int_array = "{1,2,3}";
    let string_array = "{\"a\",\"b\",\"c\"}";
    let schema = schema_with(&[
        ("intArrayCol", DataType::Utf8),
        ("stringArrayCol", DataType::Utf8),
    ]);
    let c = constraints(vec![
        ("intArrayCol", f.single(&DataType::Utf8, int_array)),
        ("stringArrayCol", f.single(&DataType::Utf8, string_array)),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"intArrayCol\", \"stringArrayCol\" FROM \"s0\".\"p0\"  WHERE (\"intArrayCol\" = ?) AND (\"stringArrayCol\" = ?)",
        &[s(int_array), s(string_array)],
    )
    .await;
}

#[test]
fn test_config_options_constructor() {
    let config_options = HashMap::from([
        ("default_database".to_string(), "test_db".to_string()),
        (
            "default".to_string(),
            "postgres://jdbc:postgresql://hostname:5432/test_db".to_string(),
        ),
    ]);
    let handler =
        PostGreSqlRecordDialect::from_config_options(Fakes::new().clients(), config_options);
    handler.unwrap();
}

#[tokio::test]
async fn build_sql_with_complex_expressions_test() {
    let f = setup();
    let schema = schema_with(&[
        ("intCol", DataType::Int32),
        ("stringCol", DataType::Utf8),
        ("floatCol", DataType::Float64),
        ("boolCol", DataType::Boolean),
    ]);
    let c = constraints(vec![
        (
            "intCol",
            f.multi(&DataType::Int32, [1, 2, 3, 5, 8].map(Value::Int).to_vec()),
        ),
        ("stringCol", f.single(&DataType::Utf8, "test%")),
        (
            "floatCol",
            f.range(&DataType::Float64, false, 1.5, true, 10.0),
        ),
        ("boolCol", f.single(&DataType::Boolean, true)),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"intCol\", \"stringCol\", \"floatCol\", \"boolCol\" FROM \"s0\".\"p0\"  WHERE (\"intCol\" IN (?,?,?,?,?)) AND (\"stringCol\" = ?) AND ((\"floatCol\" > ? AND \"floatCol\" <= ?)) AND (\"boolCol\" = ?)",
        &[
            SqlValue::Integer(1),
            SqlValue::Integer(2),
            SqlValue::Integer(3),
            SqlValue::Integer(5),
            SqlValue::Integer(8),
            s("test%"),
            SqlValue::Double(1.5),
            SqlValue::Double(10.0),
            SqlValue::Boolean(true),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_range_and_in_predicates_test() {
    let f = setup();
    let score = DataType::Decimal128(10, 2);
    let schema = schema_with(&[
        ("dateCol", DataType::Date32),
        ("category", DataType::Utf8),
        ("scoreCol", score.clone()),
    ]);
    let c = constraints(vec![
        (
            "dateCol",
            f.range(
                &DataType::Date32,
                true,
                days("2022-12-31"),
                true,
                days("2023-12-30"),
            ),
        ),
        (
            "category",
            f.multi(
                &DataType::Utf8,
                vec![Value::from("A"), Value::from("B"), Value::from("Premium")],
            ),
        ),
        (
            "scoreCol",
            f.range(&score, true, dec("85.0"), false, dec("100.0")),
        ),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"dateCol\", \"category\", \"scoreCol\" FROM \"s0\".\"p0\"  WHERE ((\"dateCol\" >= ? AND \"dateCol\" <= ?)) AND (\"category\" IN (?,?,?)) AND ((\"scoreCol\" >= ? AND \"scoreCol\" < ?))",
        &[
            SqlValue::Date(date("2022-12-31")),
            SqlValue::Date(date("2023-12-30")),
            s("A"),
            s("B"),
            s("Premium"),
            SqlValue::Decimal(dec("85.0")),
            SqlValue::Decimal(dec("100.0")),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_nullable_comparisons_test() {
    let f = setup();
    let schema = schema_with(&[
        ("nullableCol", DataType::Utf8),
        ("requiredCol", DataType::Int32),
    ]);
    let c = constraints(vec![
        ("nullableCol", f.single(&DataType::Utf8, "test")),
        ("requiredCol", f.single(&DataType::Int32, 42)),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"nullableCol\", \"requiredCol\" FROM \"s0\".\"p0\"  WHERE (\"nullableCol\" = ?) AND (\"requiredCol\" = ?)",
        &[s("test"), SqlValue::Integer(42)],
    )
    .await;
}

#[tokio::test]
async fn test_complex_expression_with_different_data_types() {
    let f = setup();
    let decimal = DataType::Decimal128(10, 2);
    let schema = schema_with(&[
        ("tinyintCol", DataType::Int8),
        ("smallintCol", DataType::Int16),
        ("bigintCol", DataType::Int64),
        ("float4Col", DataType::Float32),
        ("float8Col", DataType::Float64),
        ("decimalCol", decimal.clone()),
    ]);
    let c = constraints(vec![
        (
            "tinyintCol",
            f.range(&DataType::Int8, true, 1_i8, false, 100_i8),
        ),
        (
            "smallintCol",
            f.range(&DataType::Int16, false, 1000_i16, true, 5000_i16),
        ),
        (
            "bigintCol",
            f.range(&DataType::Int64, true, 1_000_000_i64, false, 2_000_000_i64),
        ),
        (
            "float4Col",
            f.range(&DataType::Float32, false, 1.5_f32, true, 99.9_f32),
        ),
        (
            "float8Col",
            f.range(&DataType::Float64, true, 100.001, false, 999.999),
        ),
        ("decimalCol", f.single(&decimal, dec("123.45"))),
    ]);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"tinyintCol\", \"smallintCol\", \"bigintCol\", \"float4Col\", \"float8Col\", \"decimalCol\" FROM \"s0\".\"p0\"  WHERE ((\"tinyintCol\" >= ? AND \"tinyintCol\" < ?)) AND ((\"smallintCol\" > ? AND \"smallintCol\" <= ?)) AND ((\"bigintCol\" >= ? AND \"bigintCol\" < ?)) AND ((\"float4Col\" > ? AND \"float4Col\" <= ?)) AND ((\"float8Col\" >= ? AND \"float8Col\" < ?)) AND (\"decimalCol\" = ?)",
        &[
            SqlValue::TinyInt(1),
            SqlValue::TinyInt(100),
            SqlValue::SmallInt(1000),
            SqlValue::SmallInt(5000),
            SqlValue::BigInt(1_000_000),
            SqlValue::BigInt(2_000_000),
            SqlValue::Real(1.5),
            SqlValue::Real(99.9),
            SqlValue::Double(100.001),
            SqlValue::Double(999.999),
            SqlValue::Decimal(dec("123.45")),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_limit_test() {
    let f = setup();
    let schema = schema_with(&[("id", DataType::Utf8), ("name", DataType::Utf8)]);
    let c = constraints_full(Vec::new(), Vec::new(), 100);
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"id\", \"name\" FROM \"s0\".\"p0\"  LIMIT 100",
        &[],
    )
    .await;
}

#[tokio::test]
async fn test_limit_with_different_values() {
    let f = setup();
    let schema = schema_with(&[("col1", DataType::Int32)]);
    for limit in [1_i64, 10, 50, 1000, 10000] {
        let c = constraints_full(Vec::new(), Vec::new(), limit);
        let statement = f
            .build(
                &table(),
                &schema,
                &c,
                &split(PARTITION_SCHEMA, PARTITION_NAME),
            )
            .await
            .unwrap();
        assert_eq!(
            format!("SELECT \"col1\" FROM \"s0\".\"p0\"  LIMIT {limit}"),
            statement.sql(),
            "Failed for LIMIT {limit}"
        );
    }
}

#[tokio::test]
async fn test_limit_with_constraints() {
    let f = setup();
    let schema = schema_with(&[("id", DataType::Int32), ("status", DataType::Utf8)]);
    let c = constraints_full(
        vec![
            ("id", f.range(&DataType::Int32, true, 1, false, 1000)),
            (
                "status",
                f.multi(
                    &DataType::Utf8,
                    vec![Value::from("ACTIVE"), Value::from("PENDING")],
                ),
            ),
        ],
        Vec::new(),
        25,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"id\", \"status\" FROM \"s0\".\"p0\"  WHERE ((\"id\" >= ? AND \"id\" < ?)) AND (\"status\" IN (?,?)) LIMIT 25",
        &[
            SqlValue::Integer(1),
            SqlValue::Integer(1000),
            s("ACTIVE"),
            s("PENDING"),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_order_by_test() {
    let f = setup();
    let schema = schema_with(&[("id", DataType::Utf8), ("name", DataType::Utf8)]);
    let c = constraints_full(
        Vec::new(),
        vec![OrderByField::new("name", Direction::ASC_NULLS_FIRST)],
        DEFAULT_NO_LIMIT,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"id\", \"name\" FROM \"s0\".\"p0\"  ORDER BY \"name\" ASC NULLS FIRST",
        &[],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_multiple_order_by_columns_test() {
    let f = setup();
    let schema = schema_with(&[
        ("category", DataType::Utf8),
        ("priority", DataType::Int32),
        ("created_date", DataType::Date32),
    ]);
    let c = constraints_full(
        Vec::new(),
        vec![
            OrderByField::new("category", Direction::ASC_NULLS_FIRST),
            OrderByField::new("priority", Direction::DESC_NULLS_LAST),
            OrderByField::new("created_date", Direction::DESC_NULLS_FIRST),
        ],
        DEFAULT_NO_LIMIT,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"category\", \"priority\", \"created_date\" FROM \"s0\".\"p0\"  ORDER BY \"category\" ASC NULLS FIRST, \"priority\" DESC NULLS LAST, \"created_date\" DESC NULLS FIRST",
        &[],
    )
    .await;
}

#[tokio::test]
async fn test_order_by_with_constraints() {
    let f = setup();
    let schema = schema_with(&[
        ("id", DataType::Int32),
        ("status", DataType::Utf8),
        ("score", DataType::Float64),
    ]);
    let c = constraints_full(
        vec![
            ("id", f.range(&DataType::Int32, true, 1, false, 1000)),
            (
                "status",
                f.multi(
                    &DataType::Utf8,
                    vec![Value::from("ACTIVE"), Value::from("PENDING")],
                ),
            ),
        ],
        vec![
            OrderByField::new("score", Direction::DESC_NULLS_LAST),
            OrderByField::new("id", Direction::ASC_NULLS_FIRST),
        ],
        DEFAULT_NO_LIMIT,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"id\", \"status\", \"score\" FROM \"s0\".\"p0\"  WHERE ((\"id\" >= ? AND \"id\" < ?)) AND (\"status\" IN (?,?)) ORDER BY \"score\" DESC NULLS LAST, \"id\" ASC NULLS FIRST",
        &[
            SqlValue::Integer(1),
            SqlValue::Integer(1000),
            s("ACTIVE"),
            s("PENDING"),
        ],
    )
    .await;
}

#[tokio::test]
async fn build_sql_with_all_features_test() {
    let f = setup();
    let price = DataType::Decimal128(10, 2);
    let schema = schema_with(&[
        ("id", DataType::Int32),
        ("category", DataType::Utf8),
        ("price", price.clone()),
        ("rating", DataType::Float64),
        ("active", DataType::Boolean),
    ]);
    let c = constraints_full(
        vec![
            ("id", f.range(&DataType::Int32, true, 100, false, 50000)),
            (
                "category",
                f.multi(
                    &DataType::Utf8,
                    vec![
                        Value::from("Electronics"),
                        Value::from("Books"),
                        Value::from("Clothing"),
                    ],
                ),
            ),
            (
                "price",
                f.range(&price, false, dec("10.00"), true, dec("500.00")),
            ),
            ("rating", f.range(&DataType::Float64, true, 4.0, false, 5.0)),
            ("active", f.single(&DataType::Boolean, true)),
        ],
        vec![
            OrderByField::new("category", Direction::ASC_NULLS_FIRST),
            OrderByField::new("rating", Direction::DESC_NULLS_LAST),
            OrderByField::new("price", Direction::ASC_NULLS_LAST),
        ],
        20,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"id\", \"category\", \"price\", \"rating\", \"active\" FROM \"s0\".\"p0\"  WHERE ((\"id\" >= ? AND \"id\" < ?)) AND (\"category\" IN (?,?,?)) AND ((\"price\" > ? AND \"price\" <= ?)) AND ((\"rating\" >= ? AND \"rating\" < ?)) AND (\"active\" = ?) ORDER BY \"category\" ASC NULLS FIRST, \"rating\" DESC NULLS LAST, \"price\" ASC NULLS LAST LIMIT 20",
        &[
            SqlValue::Integer(100),
            SqlValue::Integer(50000),
            s("Books"),
            s("Clothing"),
            s("Electronics"),
            SqlValue::Decimal(dec("10.00")),
            SqlValue::Decimal(dec("500.00")),
            SqlValue::Double(4.0),
            SqlValue::Double(5.0),
            SqlValue::Boolean(true),
        ],
    )
    .await;
}

#[tokio::test]
async fn test_top_n_with_order_by_scenario() {
    let f = setup();
    let salary = DataType::Decimal128(10, 2);
    let schema = schema_with(&[
        ("employee_id", DataType::Int32),
        ("department", DataType::Utf8),
        ("salary", salary.clone()),
        ("hire_date", DataType::Date32),
    ]);
    let c = constraints_full(
        vec![
            ("department", f.single(&DataType::Utf8, "Engineering")),
            (
                "salary",
                f.range(&salary, true, dec("50000"), false, dec("200000")),
            ),
        ],
        vec![
            OrderByField::new("salary", Direction::DESC_NULLS_LAST),
            OrderByField::new("hire_date", Direction::ASC_NULLS_FIRST),
        ],
        5,
    );
    assert_build(
        &f,
        &schema,
        &c,
        "SELECT \"employee_id\", \"department\", \"salary\", \"hire_date\" FROM \"s0\".\"p0\"  WHERE (\"department\" = ?) AND ((\"salary\" >= ? AND \"salary\" < ?)) ORDER BY \"salary\" DESC NULLS LAST, \"hire_date\" ASC NULLS FIRST LIMIT 5",
        &[
            s("Engineering"),
            SqlValue::Decimal(dec("50000")),
            SqlValue::Decimal(dec("200000")),
        ],
    )
    .await;
}

async fn assert_substrait_plan_generates_expected_sql(casing_filter: Option<&str>, expected: &str) {
    let f = setup();
    let schema = Schema::new(vec![
        Field::new("col1", DataType::Utf8, true),
        Field::new("col2", DataType::Int32, true),
        Field::new("col3", DataType::Utf8, true),
    ]);
    let mut properties = HashMap::from([
        (PARTITION_NAME_COL.to_string(), "*".to_string()),
        (PARTITION_SCHEMA_NAME.to_string(), "*".to_string()),
    ]);
    if let Some(filter) = casing_filter {
        properties.insert("CATALOG_CASING_FILTER".to_string(), filter.to_string());
    }
    let split = Split::new(None, None, properties);
    let c = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        -1,
        HashMap::new(),
        Some(QueryPlan::new(Some(String::new()), SUBSTRAIT_PLAN_FETCH)),
    );
    f.build(
        &TableName::new("test_schema", "test_table"),
        &schema,
        &c,
        &split,
    )
    .await
    .unwrap();
    let main: Vec<String> = f
        .connection
        .prepared_sql()
        .into_iter()
        .filter(|sql| sql != CHAR_COLUMNS_QUERY)
        .collect();
    assert_eq!(vec![expected.to_string()], main);
}

#[tokio::test]
async fn build_split_sql_substrait_plan_with_lowercase_casing_filter_generates_valid_fetch_next() {
    assert_substrait_plan_generates_expected_sql(
        Some("LOWERCASE_ONLY"),
        "SELECT *\nFROM \"test_schema\".\"test_table\"\nFETCH NEXT 10 ROWS ONLY",
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_substrait_plan_with_uppercase_casing_filter_generates_valid_fetch_next() {
    assert_substrait_plan_generates_expected_sql(
        Some("UPPERCASE_ONLY"),
        "SELECT *\nFROM \"TEST_SCHEMA\".\"TEST_TABLE\"\nFETCH NEXT 10 ROWS ONLY",
    )
    .await;
}

#[tokio::test]
async fn build_split_sql_substrait_plan_without_casing_filter_generates_valid_fetch_next() {
    assert_substrait_plan_generates_expected_sql(
        None,
        "SELECT *\nFROM \"test_schema\".\"test_table\"\nFETCH NEXT 10 ROWS ONLY",
    )
    .await;
}

#[tokio::test]
async fn test_prepare_export_statement_sets_fetch_size() {
    let f = setup();
    let statement = f
        .handler
        .dialect()
        .prepare_export_statement(&f.connection, "SELECT 1")
        .await
        .unwrap();
    assert_eq!("SELECT 1", statement.sql());
    assert_eq!(vec!["SELECT 1".to_string()], f.connection.prepared_sql());
    assert_eq!(1000, statement.fetch_size());
}

#[test]
fn test_derive_export_schema_maps_postgres_types() {
    let f = setup();
    let columns = [
        ("int4_col", JdbcType::Integer, 10, 0),
        ("numeric_col", JdbcType::Numeric, 0, 0),
        ("numeric_10_2_col", JdbcType::Numeric, 10, 2),
        ("timestamptz_col", JdbcType::Timestamp, 35, 6),
        ("time_col", JdbcType::Time, 15, 6),
        ("uuid_col", JdbcType::Other, i32::MAX, 0),
        ("array_col", JdbcType::Array, 10, 0),
    ];
    let metadata = ResultSetMetaData::new(
        columns
            .iter()
            .map(|(label, jdbc_type, precision, scale)| {
                ColumnMetadata::new(*label, *jdbc_type)
                    .with_precision(*precision)
                    .with_scale(*scale)
            })
            .collect(),
    );
    let schema = f
        .handler
        .dialect()
        .derive_export_schema(&f.handler, &metadata)
        .unwrap();
    let field_type = |name: &str| schema.field_with_name(name).unwrap().data_type().clone();
    assert_eq!(DataType::Int32, field_type("int4_col"));
    assert_eq!(DataType::Decimal128(38, 0), field_type("numeric_col"));
    assert_eq!(DataType::Decimal128(10, 2), field_type("numeric_10_2_col"));
    assert_eq!(
        DataType::Timestamp(TimeUnit::Millisecond, Some("UTC".into())),
        field_type("timestamptz_col")
    );
    assert_eq!(DataType::Utf8, field_type("time_col"));
    assert_eq!(DataType::Utf8, field_type("uuid_col"));
    assert_eq!(DataType::Utf8, field_type("array_col"));
}
