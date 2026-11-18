mod snowflake_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use indexmap::IndexMap;

use athena_federation_sdk::domain::predicate::{Constraints, QueryPlan, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::domain::split::Split;
use athena_jdbc::manager::JdbcSplitQueryBuilder;
use athena_jdbc::sql::{PreparedStatement, SqlValue};
use athena_snowflake::{SnowflakeFederationExpressionParser, SnowflakeQueryStringBuilder};

use snowflake_support::FakeConnection;

const PLAN_WHERE_ID_GREATER_THAN_10: &str = "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKZ3Q6YW55X2FueRqYARKVAQqIATqFAQoGEgQKAgIDEmUSYwoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhACGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaHhocCAEaBAoCEAIiChoIEgYKAhIAIgAiBhoECgIoChoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==";
const PLAN_WHERE_NAME_EQUALS_ABC: &str = "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRqdARKaAQqNATqKAQoGEgQKAgIDEmoSaAoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhACGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaIxohCAEaBAoCEAIiDBoKEggKBBICCAEiACIJGgcKBWIDYWJjGggSBgoCEgAiABoKEggKBBICCAEiABICSUQSBE5BTUUyCxBKKgdpc3RobXVz";
const PLAN_LIMIT_10: &str = "GnkSdwprGmkKAgoAEl86XQoGEgQKAgIDEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQAhgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGggSBgoCEgAiABoKEggKBBICCAEiABgAIAoSAklEEgROQU1FMgsQSioHaXN0aG11cw==";

fn query_builder() -> SnowflakeQueryStringBuilder {
    SnowflakeQueryStringBuilder::new(
        "\"",
        Arc::new(SnowflakeFederationExpressionParser::new("\"")),
    )
}

fn schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Int32, true),
        Field::new("name", DataType::Utf8, true),
    ])
}

async fn build_sql(substrait_plan: &str) -> (String, PreparedStatement) {
    let constraints = Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        Some(QueryPlan::new(Some(String::new()), substrait_plan)),
    );
    let spill_location = SpillLocation::S3(
        S3SpillLocation::new_builder()
            .with_bucket("bucket")
            .with_prefix("prefix")
            .build(),
    );
    let split = Split::new_builder(Some(spill_location), None).build();
    let connection = FakeConnection::new();

    let statement = query_builder()
        .build_sql(
            &connection,
            "test_catalog",
            "test_schema",
            "test_table",
            &schema(),
            &constraints,
            &split,
        )
        .await
        .unwrap();

    let prepared = connection.prepared_sql();
    assert_eq!(prepared.len(), 1);
    (prepared[0].clone(), statement)
}

#[tokio::test]
async fn test_build_split_sql_with_query_plan_success() {
    let (sql, statement) = build_sql(PLAN_WHERE_ID_GREATER_THAN_10).await;
    assert_eq!(
        sql,
        "SELECT *\nFROM \"test_schema\".\"test_table\"\nWHERE \"id\" > ?"
    );
    assert_eq!(statement.parameters(), &[SqlValue::Integer(10)]);
}

#[tokio::test]
async fn test_build_split_sql_with_parameter_binding() {
    let (sql, statement) = build_sql(PLAN_WHERE_NAME_EQUALS_ABC).await;
    assert_eq!(
        sql,
        "SELECT *\nFROM \"test_schema\".\"test_table\"\nWHERE \"name\" = ?"
    );
    assert_eq!(
        statement.parameters(),
        &[SqlValue::String("abc".to_string())]
    );
}

#[tokio::test]
async fn test_build_split_sql_with_query_plan_no_where_clause() {
    let (sql, statement) = build_sql(PLAN_LIMIT_10).await;
    assert_eq!(
        sql,
        "SELECT *\nFROM \"test_schema\".\"test_table\"\nLIMIT 10"
    );
    assert!(statement.parameters().is_empty());
}

const PLAN_CHARACTER_CASTS: &str = "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSFRoTCAEQARoNZXF1YWw6YW55X2FueRq9ARK6AQqxATquAQoGEgQKAgIDEnIScAoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhABGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaKxopCAEaBAoCEAIiFhoUWhIKBGICEAISCBIGCgISACIAGAIiBxoFCgNiATUaF1oVCgeyAQQIChgCEggSBgoCEgAiABgCGhdaFQoHqgEECAMYAhIIEgYKAhIAIgAYAhIBQxIBRDILEEoqB2lzdGhtdXM=";

#[tokio::test]
async fn test_build_split_sql_character_casts_have_no_character_set() {
    let (sql, _) = build_sql(PLAN_CHARACTER_CASTS).await;
    assert_eq!(
        "SELECT CAST(\"id\" AS VARCHAR(10)) AS \"id0\", CAST(\"id\" AS CHAR(3)) AS \"id1\"\nFROM \"test_schema\".\"test_table\"\nWHERE CAST(\"id\" AS VARCHAR) = '5'",
        sql
    );
}
