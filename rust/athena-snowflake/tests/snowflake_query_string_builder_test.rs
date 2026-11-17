mod snowflake_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use indexmap::IndexMap;

use athena_federation_sdk::connection::EnvironmentConstants;
use athena_federation_sdk::data::BlockAllocatorImpl;
use athena_federation_sdk::domain::predicate::{
    Constraints, Direction, OrderByField, Range, SortedRangeSet, ValueSet,
};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_federation_substrait::{DatabaseProduct, SqlDialect};
use athena_jdbc::manager::JdbcSplitQueryBuilder;
use athena_jdbc::sql::{PreparedStatement, SqlValue};
use athena_snowflake::{
    SnowflakeDialect, SnowflakeFederationExpressionParser, SnowflakeQueryStringBuilder,
};

use snowflake_support::FakeConnection;

const QUOTE_CHARACTER: &str = "\"";

fn query_builder() -> SnowflakeQueryStringBuilder {
    let expression_parser = SnowflakeFederationExpressionParser::new(QUOTE_CHARACTER);
    SnowflakeQueryStringBuilder::new(QUOTE_CHARACTER, Arc::new(expression_parser))
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
            "testCatalog",
            "testSchema",
            "testTable",
            schema,
            constraints,
            split,
        )
        .await
        .unwrap()
}

fn base_export_sql(catalog: &str, schema: &Schema, constraints: &Constraints) -> String {
    query_builder()
        .get_base_export_sql_string(catalog, "testSchema", "testTable", schema, constraints)
        .unwrap()
        .unwrap()
}

#[test]
fn test_get_from_clause_with_split() {
    let result = query_builder().get_from_clause_with_split(
        "testCatalog",
        "testSchema",
        "testTable",
        &empty_split(),
    );
    assert!(result.contains("\"testSchema\""));
    assert!(result.contains("\"testTable\""));
    assert_eq!(result, " FROM \"testSchema\".\"testTable\" ");
}

#[test]
fn test_get_from_clause_with_split_no_schema() {
    let result =
        query_builder().get_from_clause_with_split("testCatalog", "", "testTable", &empty_split());
    assert!(result.contains("\"testTable\""));
    assert!(!result.contains("null"));
    assert_eq!(result, " FROM \"testTable\" ");
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"  WHERE (\"col2\" = ?)ORDER BY col1  LIMIT 1000 OFFSET 0";
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"  ORDER BY \"col1\" ASC NULLS FIRST, \"col2\" DESC NULLS LASTORDER BY col1  LIMIT 1000 OFFSET 0";
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"  LIMIT 100";
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ";
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

    let result = base_export_sql("testCatalog", &schema, &empty_constraints());

    assert!(result.contains("SELECT"));
    assert!(result.contains("\"col1\""));
    assert!(result.contains("\"col2\""));
    assert!(result.contains("FROM"));
    assert!(result.contains("\"testSchema\".\"testTable\""));
    assert!(!result.contains("\"partition\""));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" "
    );
}

#[test]
fn test_get_base_export_sql_string_with_constraints() {
    let allocator = BlockAllocatorImpl::new();
    let range = Range::greater_than(&allocator, &DataType::Int32, 10_i32).unwrap();
    let constraints = constraints(int_summary(range), Vec::new(), 100);

    let result = base_export_sql("testCatalog", &two_column_schema(), &constraints);

    assert!(result.contains("WHERE"));
    assert!(result.contains("LIMIT"));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"  WHERE ((\"col2\" > 10)) LIMIT 100"
    );
}

#[test]
fn test_get_base_export_sql_string_with_order_by() {
    let order_by = vec![OrderByField::new("col1", Direction::ASC_NULLS_FIRST)];
    let constraints = constraints(IndexMap::new(), order_by, -1);

    let result = base_export_sql("testCatalog", &two_column_schema(), &constraints);

    assert!(result.contains("ORDER BY"));
    assert!(result.contains("\"col1\""));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\"  ORDER BY \"col1\" ASC NULLS FIRST"
    );
}

#[test]
fn test_get_base_export_sql_string_no_catalog() {
    let result = base_export_sql("", &two_column_schema(), &empty_constraints());

    assert!(result.contains("\"testSchema\".\"testTable\""));
    assert!(!result.contains("null"));
    assert_eq!(
        result,
        "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" "
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ORDER BY \"id\",\"name\"  LIMIT 5000 OFFSET 10000";
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

    let expected = "SELECT \"col1\", \"col2\" FROM \"testSchema\".\"testTable\" ";
    assert_eq!(connection.prepared_sql(), vec![expected.to_string()]);
    assert_eq!(statement.sql(), expected);
    assert!(statement.parameters().is_empty());
}

#[test]
fn test_append_limit_offset_with_null_split() {
    assert_eq!(
        query_builder().append_limit_offset_with_optional_split(None),
        ""
    );
}

#[test]
fn test_append_limit_offset_with_empty_split() {
    assert_eq!(query_builder().append_limit_offset(&empty_split()), "");
}

#[test]
fn test_append_limit_offset_with_missing_markers() {
    let split = partition_split("some-random-value");
    assert_eq!(query_builder().append_limit_offset(&split), "");
}

#[test]
fn test_append_limit_offset_with_malformed_partition() {
    let split = partition_split("partition-offset-0-limit-100-primary-col1");
    assert_eq!(query_builder().append_limit_offset(&split), "");
}

#[test]
fn test_append_limit_offset_with_empty_primary_key() {
    let split = partition_split("partition-primary--limit-1000-offset-0");
    assert_eq!(query_builder().append_limit_offset(&split), "");
}

#[test]
fn test_append_limit_offset_with_non_numeric_limit() {
    let split = partition_split("partition-primary-col1-limit-abc-offset-0");
    assert_eq!(query_builder().append_limit_offset(&split), "");
}

#[test]
fn test_append_limit_offset_with_non_numeric_offset() {
    let split = partition_split("partition-primary-col1-limit-1000-offset-xyz");
    assert_eq!(query_builder().append_limit_offset(&split), "");
}

#[test]
fn test_append_limit_offset_with_valid_partition() {
    let split = partition_split("partition-primary-\"id\"-limit-1000-offset-500");
    assert_eq!(
        query_builder().append_limit_offset(&split),
        "ORDER BY \"id\"  LIMIT 1000 OFFSET 500"
    );
}

#[test]
fn test_append_limit_offset_with_composite_primary_key() {
    let split = partition_split("partition-primary-\"col1\",\"col2\"-limit-5000-offset-10000");
    assert_eq!(
        query_builder().append_limit_offset(&split),
        "ORDER BY \"col1\",\"col2\"  LIMIT 5000 OFFSET 10000"
    );
}

#[test]
fn test_append_limit_offset_with_casing_filter_before_partition() {
    let properties = HashMap::from([
        (
            EnvironmentConstants::CATALOG_CASING_FILTER.to_string(),
            EnvironmentConstants::UPPERCASE_ONLY.to_string(),
        ),
        (
            "partition".to_string(),
            "partition-primary-\"id\"-limit-100-offset-200".to_string(),
        ),
    ]);
    let split = Split::new(Some(spill_location()), None, properties);

    let result = query_builder().append_limit_offset(&split);

    assert!(result.contains("ORDER BY \"id\""));
    assert!(result.contains("LIMIT 100 OFFSET 200"));
    assert_eq!(result, "ORDER BY \"id\"  LIMIT 100 OFFSET 200");
}

#[test]
fn test_get_sql_dialect() {
    let dialect = query_builder().get_sql_dialect();
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SqlDialect::snowflake());
    assert_eq!(dialect, SnowflakeDialect::default_dialect());
}

#[test]
fn test_get_sql_dialect_with_casing_filter() {
    let dialect = query_builder().get_sql_dialect_with_catalog_casing_filter(true);
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SnowflakeDialect::new(true).into_sql_dialect());
    assert!(dialect.is_upper_casing());
}

#[test]
fn test_get_sql_dialect_with_casing_filter_false() {
    let dialect = query_builder().get_sql_dialect_with_catalog_casing_filter(false);
    assert_eq!(dialect.database_product(), DatabaseProduct::Snowflake);
    assert_eq!(dialect, SnowflakeDialect::new(false).into_sql_dialect());
    assert!(!dialect.is_upper_casing());
}
