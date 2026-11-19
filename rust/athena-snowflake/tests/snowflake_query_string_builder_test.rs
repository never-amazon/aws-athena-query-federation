mod snowflake_support;

use athena_federation_sdk::data::block_utils;
use std::collections::HashMap;

use arrow::datatypes::{DataType, Field, Schema};
use indexmap::IndexMap;

use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::expression::{
    ConstantExpression, FederationExpression, FunctionCallExpression, VariableExpression,
    DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
};
use athena_federation_sdk::domain::predicate::functions::StandardFunctions;
use athena_federation_sdk::domain::predicate::{
    Constraints, Direction, OrderByField, Range, SortedRangeSet, ValueSet,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_substrait::DatabaseProduct;
use athena_jdbc::manager::{SplitDialect, SplitQuery, SplitQueryBuilder, TypeAndValue};
use athena_jdbc::sql::{PreparedStatement, SqlValue};
use athena_snowflake::{
    append_order_by_limit_offset, base_export_sql, expand_sql, SnowflakeDialect,
    SnowflakeQueryStringBuilder, SnowflakeSplitDialect,
};

use snowflake_support::FakeConnection;

const QUOTE_CHARACTER: &str = "\"";

fn query_builder() -> SnowflakeQueryStringBuilder {
    SplitQueryBuilder::new(QUOTE_CHARACTER, SnowflakeSplitDialect)
}

fn spill_location() -> SpillLocation {
    SpillLocation::S3(
        S3SpillLocation::new_builder()
            .with_bucket("test")
            .with_prefix("test")
            .build(),
    )
}

fn empty_split() -> Split {
    Split::new_builder(Some(spill_location()), None).build()
}

fn partition_split(partition: &str) -> Split {
    Split::new_builder(Some(spill_location()), None)
        .add("partition", partition)
        .build()
}

fn two_column_schema() -> Schema {
    Schema::new(vec![
        Field::new("col1", DataType::Utf8, true),
        Field::new("col2", DataType::Int32, true),
    ])
}

fn constraints(
    summary: IndexMap<String, ValueSet>,
    order_by: Vec<OrderByField>,
    limit: i64,
) -> Constraints {
    Constraints::new(summary, Vec::new(), order_by, limit, HashMap::new(), None)
}

fn empty_constraints() -> Constraints {
    constraints(IndexMap::new(), Vec::new(), -1)
}

fn int_summary(range: Range) -> IndexMap<String, ValueSet> {
    let value_set = SortedRangeSet::copy_of(&DataType::Int32, [range], false).unwrap();
    IndexMap::from([("col2".to_string(), ValueSet::from(value_set))])
}

async fn build(
    connection: &FakeConnection,
    schema: &Schema,
    constraints: &Constraints,
    split: &Split,
) -> PreparedStatement {
    query_builder()
        .build_sql(
            connection,
            &SplitQuery {
                catalog_name: "testCatalog",
                table_name: &TableName::new("testSchema", "testTable"),
                schema,
                constraints,
                split,
            },
        )
        .await
        .unwrap()
}

fn base_export(_catalog: &str, schema: &Schema, constraints: &Constraints) -> String {
    base_export_sql(
        &query_builder(),
        &TableName::new("testSchema", "testTable"),
        schema,
        constraints,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn test_get_from_clause_with_split() {
    let builder = query_builder();
    let result = builder.dialect().table_clause(
        &builder,
        &TableName::new("testSchema", "testTable"),
        &empty_split(),
    );
    assert!(result.contains("\"testSchema\""));
    assert!(result.contains("\"testTable\""));
    assert_eq!(result, " FROM \"testSchema\".\"testTable\"");
}

#[test]
fn test_get_from_clause_with_split_no_schema() {
    let builder = query_builder();
    let result =
        builder
            .dialect()
            .table_clause(&builder, &TableName::new("", "testTable"), &empty_split());
    assert!(result.contains("\"testTable\""));
    assert!(!result.contains("null"));
    assert_eq!(result, " FROM \"testTable\"");
}

#[test]
fn test_quote() {
    assert_eq!(
        query_builder().quote("testIdentifier"),
        "\"testIdentifier\""
    );
}

#[tokio::test]
async fn test_build_sql_with_simple_constraints() {
    let allocator = BlockAllocatorImpl::new();
    let connection = FakeConnection::new();
    let range = Range::equal(&allocator, &DataType::Int32, 42_i32).unwrap();
    let constraints = constraints(int_summary(range), Vec::new(), -1);
    let split = partition_split("partition-primary-col1-limit-1000-offset-0");

    let statement = build(&connection, &two_column_schema(), &constraints, &split).await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE (\"col2\" = ?) ORDER BY col1 LIMIT 1000 OFFSET 0";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert_eq!(statement.parameters(), &[SqlValue::Integer(42)]);
}

#[tokio::test]
async fn test_build_sql_with_order_by() {
    let connection = FakeConnection::new();
    let order_by = vec![
        OrderByField::new("col1", Direction::ASC_NULLS_FIRST),
        OrderByField::new("col2", Direction::DESC_NULLS_LAST),
    ];
    let constraints = constraints(IndexMap::new(), order_by, -1);
    let split = partition_split("partition-primary-col1-limit-1000-offset-0");

    let statement = build(&connection, &two_column_schema(), &constraints, &split).await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST, \"col2\" DESC NULLS LAST, col1 LIMIT 1000 OFFSET 0";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[tokio::test]
async fn test_build_sql_with_limit() {
    let connection = FakeConnection::new();
    let constraints = constraints(IndexMap::new(), Vec::new(), 100);
    let split = partition_split("test-partition");

    let statement = build(&connection, &two_column_schema(), &constraints, &split).await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" LIMIT 100";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[tokio::test]
async fn test_build_sql_with_partition_constraints() {
    let connection = FakeConnection::new();
    let split = partition_split("partition-primary--limit-1000-offset-0");

    let statement = build(
        &connection,
        &two_column_schema(),
        &empty_constraints(),
        &split,
    )
    .await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[test]
fn test_get_base_export_sql_string() {
    let schema = Schema::new(vec![
        Field::new("col1", DataType::Utf8, true),
        Field::new("col2", DataType::Int32, true),
        Field::new("partition", DataType::Utf8, true),
    ]);

    let result = base_export("testCatalog", &schema, &empty_constraints());

    assert!(result.contains("SELECT"));
    assert!(result.contains("\"col1\""));
    assert!(result.contains("\"col2\""));
    assert!(result.contains("FROM"));
    assert!(result.contains("\"testSchema\".\"testTable\""));
    assert!(!result.contains("\"partition\""));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\""
    );
}

#[test]
fn test_get_base_export_sql_string_with_constraints() {
    let allocator = BlockAllocatorImpl::new();
    let range = Range::greater_than(&allocator, &DataType::Int32, 10_i32).unwrap();
    let constraints = constraints(int_summary(range), Vec::new(), 100);

    let result = base_export("testCatalog", &two_column_schema(), &constraints);

    assert!(result.contains("WHERE"));
    assert!(result.contains("LIMIT"));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE ((\"col2\" > 10)) LIMIT 100"
    );
}

#[test]
fn test_get_base_export_sql_string_with_order_by() {
    let order_by = vec![OrderByField::new("col1", Direction::ASC_NULLS_FIRST)];
    let constraints = constraints(IndexMap::new(), order_by, -1);

    let result = base_export("testCatalog", &two_column_schema(), &constraints);

    assert!(result.contains("ORDER BY"));
    assert!(result.contains("\"col1\""));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST"
    );
}

#[test]
fn test_get_base_export_sql_string_no_catalog() {
    let result = base_export("", &two_column_schema(), &empty_constraints());

    assert!(result.contains("\"testSchema\".\"testTable\""));
    assert!(!result.contains("null"));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\""
    );
}

#[tokio::test]
async fn test_build_sql_with_complex_partition() {
    let connection = FakeConnection::new();
    let split = partition_split("partition-primary-\"id\",\"name\"-limit-5000-offset-10000");

    let statement = build(
        &connection,
        &two_column_schema(),
        &empty_constraints(),
        &split,
    )
    .await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"id\",\"name\" LIMIT 5000 OFFSET 10000";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[tokio::test]
async fn test_build_sql_with_all_partition() {
    let connection = FakeConnection::new();
    let split = partition_split("*");

    let statement = build(
        &connection,
        &two_column_schema(),
        &empty_constraints(),
        &split,
    )
    .await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[test]
fn test_append_limit_offset_with_null_split() {
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), None, &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_empty_split() {
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&empty_split()), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_missing_markers() {
    let split = partition_split("some-random-value");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_malformed_partition() {
    let split = partition_split("partition-offset-0-limit-100-primary-col1");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_empty_primary_key() {
    let split = partition_split("partition-primary--limit-1000-offset-0");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_non_numeric_limit() {
    let split = partition_split("partition-primary-col1-limit-abc-offset-0");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_non_numeric_offset() {
    let split = partition_split("partition-primary-col1-limit-1000-offset-xyz");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_valid_partition() {
    let split = partition_split("partition-primary-\"id\"-limit-1000-offset-500");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        " ORDER BY \"id\" LIMIT 1000 OFFSET 500"
    );
}

#[test]
fn test_append_limit_offset_with_composite_primary_key() {
    let split = partition_split("partition-primary-\"col1\",\"col2\"-limit-5000-offset-10000");
    assert_eq!(
        append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints()),
        " ORDER BY \"col1\",\"col2\" LIMIT 5000 OFFSET 10000"
    );
}

#[test]
fn test_append_limit_offset_with_casing_filter_before_partition() {
    let properties = HashMap::from([
        (
            environment_constants::CATALOG_CASING_FILTER.to_string(),
            environment_constants::UPPERCASE_ONLY.to_string(),
        ),
        (
            "partition".to_string(),
            "partition-primary-\"id\"-limit-100-offset-200".to_string(),
        ),
    ]);
    let split = Split::new(Some(spill_location()), None, properties);

    let result = append_order_by_limit_offset(&query_builder(), Some(&split), &empty_constraints());

    assert!(result.contains("ORDER BY \"id\""));
    assert!(result.contains("LIMIT 100 OFFSET 200"));
    assert_eq!(result, " ORDER BY \"id\" LIMIT 100 OFFSET 200");
}

#[tokio::test]
async fn test_build_sql_with_order_by_limit_and_pagination() {
    let allocator = BlockAllocatorImpl::new();
    let connection = FakeConnection::new();
    let range = Range::greater_than(&allocator, &DataType::Int32, 10_i32).unwrap();
    let order_by = vec![OrderByField::new("col2", Direction::DESC_NULLS_LAST)];
    let constraints = constraints(int_summary(range), order_by, 5000);
    let split = partition_split("partition-primary-\"id\"-limit-1000-offset-3000");

    let statement = build(&connection, &two_column_schema(), &constraints, &split).await;

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" WHERE ((\"col2\" > ?)) ORDER BY \"col2\" DESC NULLS LAST, \"id\" LIMIT 1000 OFFSET 3000";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.parameters(), &[SqlValue::Integer(10)]);
}

#[tokio::test]
async fn test_build_sql_with_limit_below_page_size_and_pagination() {
    let connection = FakeConnection::new();
    let constraints = constraints(IndexMap::new(), Vec::new(), 100);
    let split = partition_split("partition-primary-\"id\"-limit-1000-offset-2000");

    build(&connection, &two_column_schema(), &constraints, &split).await;

    assert_eq!(
        connection.prepared_sql(),
        vec![
            "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"id\" LIMIT 100 OFFSET 2000"
                .to_string()
        ]
    );
}

#[test]
fn test_get_base_export_sql_string_with_order_by_and_limit() {
    let order_by = vec![OrderByField::new("col1", Direction::ASC_NULLS_FIRST)];
    let constraints = constraints(IndexMap::new(), order_by, 10);

    let result = base_export("testCatalog", &two_column_schema(), &constraints);

    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"col1\" ASC NULLS FIRST LIMIT 10"
    );
}

#[test]
fn test_get_sql_dialect() {
    let dialect = query_builder().dialect().sql_dialect(None);
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SnowflakeDialect::new(false).into_sql_dialect());
    assert!(!dialect.supports_char_set());
}

#[test]
fn test_get_sql_dialect_with_casing_filter() {
    let dialect = query_builder().dialect().sql_dialect(Some(true));
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SnowflakeDialect::new(true).into_sql_dialect());
    assert!(dialect.is_upper_casing());
}

#[test]
fn test_get_sql_dialect_with_casing_filter_false() {
    let dialect = query_builder().dialect().sql_dialect(Some(false));
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SnowflakeDialect::new(false).into_sql_dialect());
    assert!(!dialect.is_upper_casing());
}

fn constant(
    allocator: &BlockAllocatorImpl,
    value: Value,
    arrow_type: DataType,
) -> FederationExpression {
    let block = block_utils::new_block(
        allocator,
        DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
        &arrow_type,
        vec![value],
    )
    .unwrap();
    FederationExpression::Constant(ConstantExpression::new(Some(block), arrow_type))
}

fn function(name: StandardFunctions, arguments: Vec<FederationExpression>) -> FederationExpression {
    FederationExpression::FunctionCall(FunctionCallExpression::new(
        DataType::Boolean,
        name.function_name(),
        arguments,
    ))
}

fn complex_expression_constraints(allocator: &BlockAllocatorImpl) -> Constraints {
    let col1 =
        || FederationExpression::Variable(VariableExpression::new("col1", DataType::Utf8).unwrap());
    let col2 =
        FederationExpression::Variable(VariableExpression::new("col2", DataType::Int32).unwrap());
    let greater = function(
        StandardFunctions::GreaterThan,
        vec![
            col2,
            constant(allocator, Value::from(10_i32), DataType::Int32),
        ],
    );
    let equal = function(
        StandardFunctions::Equal,
        vec![
            col1(),
            constant(allocator, Value::from("it's"), DataType::Utf8),
        ],
    );
    let is_null = function(StandardFunctions::IsNull, vec![col1()]);
    let or = function(StandardFunctions::Or, vec![equal, is_null]);
    let range = Range::less_than_or_equal(allocator, &DataType::Int32, 100_i32).unwrap();
    Constraints::new(
        int_summary(range),
        vec![greater, or],
        vec![OrderByField::new("col1", Direction::DESC_NULLS_LAST)],
        5,
        HashMap::new(),
        None,
    )
}

#[test]
fn test_build_sql_string_literal_with_complex_expressions() {
    let allocator = BlockAllocatorImpl::new();
    let constraints = complex_expression_constraints(&allocator);
    let split = partition_split("partition-primary-\"col2\"-limit-3-offset-6");
    let mut accumulator = Vec::new();

    let sql = query_builder()
        .build_sql_string_literal(
            &SplitQuery {
                catalog_name: "cat",
                table_name: &TableName::new("sch", "tbl"),
                schema: &two_column_schema(),
                constraints: &constraints,
                split: &split,
            },
            "\"col1\", \"col2\"",
            &mut accumulator,
        )
        .unwrap();

    assert_eq!(
        sql,
        "SELECT \"col1\", \"col2\" FROM \"sch\".\"tbl\" WHERE ((\"col2\" <= ?)) AND (\"col2\" > ?) AND ((\"col1\" = ?) OR (\"col1\" IS NULL)) ORDER BY \"col1\" DESC NULLS LAST, \"col2\" LIMIT 3 OFFSET 6"
    );
    let parameters: Vec<(DataType, String)> = accumulator
        .iter()
        .map(|parameter| {
            (
                parameter.get_type().clone(),
                parameter.value().java_to_string(),
            )
        })
        .collect();
    assert_eq!(
        parameters,
        vec![
            (DataType::Int32, "100".to_string()),
            (DataType::Int32, "10".to_string()),
            (DataType::Utf8, "it's".to_string()),
        ]
    );
}

#[test]
fn test_get_base_export_sql_string_with_complex_expressions() {
    let allocator = BlockAllocatorImpl::new();
    let constraints = complex_expression_constraints(&allocator);

    let result = base_export_sql(
        &query_builder(),
        &TableName::new("sch", "tbl"),
        &two_column_schema(),
        &constraints,
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"sch\".\"tbl\" WHERE ((\"col2\" <= 100)) AND (\"col2\" > 10) AND ((\"col1\" = 'it''s') OR (\"col1\" IS NULL)) ORDER BY \"col1\" DESC NULLS LAST LIMIT 5"
    );
}

fn varchar(value: &str) -> TypeAndValue {
    TypeAndValue::new(DataType::Utf8, Value::String(value.to_string())).unwrap()
}

#[test]
fn test_expand_sql_keeps_era_and_sign_in_date_and_timestamp_literals() {
    let sql = r#"SELECT * FROM "t" WHERE "d" = ? AND "e" = ? AND "ts" = ?"#;
    let values = [
        TypeAndValue::new(DataType::Date32, Value::Int(-735_160)).unwrap(),
        TypeAndValue::new(DataType::Date32, Value::Int(2_932_897)).unwrap(),
        TypeAndValue::new(DataType::Date64, Value::Long(-63_517_780_800_000)).unwrap(),
    ];
    assert_eq!(
        Some(
            r#"SELECT * FROM "t" WHERE "d" = DATE '-0043-03-15' AND "e" = DATE '+10000-01-01' AND "ts" = TIMESTAMP '-0043-03-15T12:00:00Z'"#
                .to_string()
        ),
        expand_sql(sql, &values).unwrap()
    );
}

#[test]
fn test_expand_sql_escapes_backslash_and_quote() {
    let sql = r#"SELECT * FROM "t" WHERE "a" = ? AND "b" = ? AND "c" = ?"#;
    let values = [varchar("x\\"), varchar("\\' OR 1=1 --"), varchar("it's")];
    assert_eq!(
        Some(
            r#"SELECT * FROM "t" WHERE "a" = 'x\\' AND "b" = '\\'' OR 1=1 --' AND "c" = 'it''s'"#
                .to_string()
        ),
        expand_sql(sql, &values).unwrap()
    );
}

#[test]
fn test_expand_sql_keeps_dollar_and_backslash_in_values_literal() {
    let sql = r#"SELECT * FROM "t" WHERE "a" = ? AND "b" = ?"#;
    let values = [varchar("$1"), varchar("a\\$0b")];
    assert_eq!(
        Some(r#"SELECT * FROM "t" WHERE "a" = '$1' AND "b" = 'a\\$0b'"#.to_string()),
        expand_sql(sql, &values).unwrap()
    );
}

#[test]
fn test_expand_sql_keeps_question_marks_inside_literals_and_identifiers() {
    let sql = r#"SELECT "q?" FROM "t" WHERE "a" = ? AND "b" = '?' AND "c" = ?"#;
    let values = [varchar("a?b"), varchar("c")];
    assert_eq!(
        Some(r#"SELECT "q?" FROM "t" WHERE "a" = 'a?b' AND "b" = '?' AND "c" = 'c'"#.to_string()),
        expand_sql(sql, &values).unwrap()
    );
}

#[tokio::test]
async fn bind_timestamp_and_date_send_exact_epoch_values_through_the_driver() {
    let timestamp = chrono::NaiveDate::from_ymd_opt(1582, 9, 29)
        .unwrap()
        .and_hms_milli_opt(6, 2, 56, 643)
        .unwrap();
    assert_exact_binds(
        timestamp,
        chrono::NaiveDate::from_ymd_opt(-43, 3, 15).unwrap(),
    )
    .await;
}

#[tokio::test]
async fn bind_timestamp_and_date_send_exact_epoch_values_before3500_bc() {
    let timestamp = chrono::NaiveDate::from_ymd_opt(-5000, 1, 1)
        .unwrap()
        .and_hms_opt(0, 30, 0)
        .unwrap();
    assert_exact_binds(
        timestamp,
        chrono::NaiveDate::from_ymd_opt(-5000, 1, 1).unwrap(),
    )
    .await;
}

async fn assert_exact_binds(timestamp: chrono::NaiveDateTime, date: chrono::NaiveDate) {
    let allocator = BlockAllocatorImpl::new();
    let connection = FakeConnection::new();
    let schema = Schema::new(vec![
        Field::new("ts", DataType::Date64, true),
        Field::new("d", DataType::Date32, true),
    ]);
    let epoch_day = date
        .signed_duration_since(chrono::NaiveDate::default())
        .num_days();
    let summary = IndexMap::from([
        (
            "ts".to_string(),
            ValueSet::from(
                SortedRangeSet::copy_of(
                    &DataType::Date64,
                    [Range::equal(
                        &allocator,
                        &DataType::Date64,
                        Value::LocalDateTime(timestamp),
                    )
                    .unwrap()],
                    false,
                )
                .unwrap(),
            ),
        ),
        (
            "d".to_string(),
            ValueSet::from(
                SortedRangeSet::copy_of(
                    &DataType::Date32,
                    [Range::equal(
                        &allocator,
                        &DataType::Date32,
                        i32::try_from(epoch_day).unwrap(),
                    )
                    .unwrap()],
                    false,
                )
                .unwrap(),
            ),
        ),
    ]);
    let constraints = constraints(summary, Vec::new(), -1);

    let statement = build(&connection, &schema, &constraints, &empty_split()).await;

    assert_eq!(
        statement.parameters(),
        &[SqlValue::Timestamp(timestamp), SqlValue::Date(date)]
    );
}
