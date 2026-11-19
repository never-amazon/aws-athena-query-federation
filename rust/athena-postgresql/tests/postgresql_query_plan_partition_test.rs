#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use std::collections::{HashMap, HashSet};

use arrow::datatypes::{DataType, Field, Schema};
use indexmap::IndexMap;
use rusqlite::types::Value as SqliteValue;
use rusqlite::Connection;

use athena_federation_sdk::domain::predicate::{Constraints, QueryPlan, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::{S3SpillLocation, SpillLocation, Split, TableName};
use athena_jdbc::manager::SplitQuery;
use athena_jdbc::sql::SqlValue;
use athena_postgresql::{
    query_string_builder, PostGreSqlQueryStringBuilder, BLOCK_PARTITION_COLUMN_NAME,
    BLOCK_PARTITION_SCHEMA_COLUMN_NAME,
};

use jdbc_support::FakeConnection;

const ROW_COUNT: i64 = 23;
const CHILDREN: [(&str, i64, i64); 3] = [
    ("test_table_low", 1, 7),
    ("test_table_mid", 8, 15),
    ("test_table_high", 16, 23),
];
const PLAIN: &str = "SELECT id, name FROM test_table";
const OR_FILTER: &str = "SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'";

const PLANS: [(&str, &str); 4] = [
    (
        PLAIN,
        "Gm0SawpfOl0KBhIECgICAxI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==",
    ),
    (
        "SELECT id, name FROM test_table WHERE id > 10",
        "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSEhoQCAEQARoKZ3Q6YW55X2FueRqYARKVAQqIATqFAQoGEgQKAgIDEmUSYwoCCgASPQo7CgIKABIaCgJpZAoEbmFtZRIOCgQqAhACCgRiAhABGAI6GQoLdGVzdF9zY2hlbWEKCnRlc3RfdGFibGUaHhocCAEaBAoCEAIiChoIEgYKAhIAIgAiBhoECgIoChoIEgYKAhIAIgAaChIICgQSAggBIgASAklEEgROQU1FMgsQSioHaXN0aG11cw==",
    ),
    (
        OR_FILTER,
        "ChsIARIXL2Z1bmN0aW9uc19ib29sZWFuLnlhbWwKHggCEhovZnVuY3Rpb25zX2NvbXBhcmlzb24ueWFtbBIPGg0IARABGgdvcjpib29sEhIaEAgCEAIaCmx0OmFueV9hbnkSFRoTCAIQAxoNZXF1YWw6YW55X2FueRrPARLMAQq/ATq8AQoGEgQKAgIDEpsBEpgBCgIKABI9CjsKAgoAEhoKAmlkCgRuYW1lEg4KBCoCEAIKBGICEAEYAjoZCgt0ZXN0X3NjaGVtYQoKdGVzdF90YWJsZRpTGlEIARoECgIQASIgGh4aHAgCGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAUiJRojGiEIAxoECgIQASIMGgoSCAoEEgIIASIAIgkaBwoFYgNuMTcaCBIGCgISACIAGgoSCAoEEgIIASIAEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=",
    ),
    (
        "SELECT name FROM test_table WHERE id >= 3",
        "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSExoRCAEQARoLZ3RlOmFueV9hbnkahwEShAEKfDp6CgUSAwoBAhJlEmMKAgoAEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGh4aHAgBGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAMaChIICgQSAggBIgASBE5BTUUyCxBKKgdpc3RobXVz",
    ),
];

fn query_builder() -> PostGreSqlQueryStringBuilder {
    query_string_builder()
}

fn schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Int32, true),
        Field::new("name", DataType::Utf8, true),
    ])
}

fn constraints(plan: Option<&str>) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        plan.map(|plan| QueryPlan::new(Some(String::new()), plan)),
    )
}

fn split(partition_schema: &str, partition_name: &str) -> Split {
    Split::new_builder(
        Some(SpillLocation::S3(
            S3SpillLocation::new_builder()
                .with_bucket("bucket")
                .with_prefix("prefix")
                .build(),
        )),
        None,
    )
    .add(BLOCK_PARTITION_SCHEMA_COLUMN_NAME, partition_schema)
    .add(BLOCK_PARTITION_COLUMN_NAME, partition_name)
    .build()
}

fn load_tables() -> Connection {
    let database = Connection::open_in_memory().unwrap();
    database
        .execute_batch(
            "ATTACH DATABASE ':memory:' AS \"test_schema\";
             CREATE TABLE \"test_schema\".\"test_table\" (\"id\" INTEGER PRIMARY KEY, \"name\" TEXT);",
        )
        .unwrap();
    for id in 1..=ROW_COUNT {
        database
            .execute(
                "INSERT INTO \"test_schema\".\"test_table\" VALUES (?1, ?2)",
                rusqlite::params![id, format!("n{id}")],
            )
            .unwrap();
    }
    for (child, low, high) in CHILDREN {
        database
            .execute(
                &format!("CREATE TABLE \"test_schema\".\"{child}\" AS SELECT * FROM \"test_schema\".\"test_table\" WHERE \"id\" BETWEEN ?1 AND ?2"),
                rusqlite::params![low, high],
            )
            .unwrap();
    }
    database
}

async fn build_sql(plan: Option<&str>, split: &Split) -> (String, Vec<SqlValue>) {
    let connection = FakeConnection::new();
    let table_name = TableName::new("test_schema", "test_table");
    let statement = athena_postgresql::build_sql(
        &query_builder(),
        &connection,
        &SplitQuery {
            catalog_name: "",
            table_name: &table_name,
            schema: &schema(),
            constraints: &constraints(plan),
            split,
        },
    )
    .await
    .unwrap();
    (statement.sql().to_string(), statement.parameters().to_vec())
}

fn sqlite_value(value: &SqlValue) -> SqliteValue {
    match value {
        SqlValue::Integer(v) => SqliteValue::Integer(i64::from(*v)),
        SqlValue::BigInt(v) => SqliteValue::Integer(*v),
        SqlValue::String(v) => SqliteValue::Text(v.clone()),
        other => panic!("unexpected bind value {other:?}"),
    }
}

async fn run(database: &Connection, plan: Option<&str>, split: &Split) -> Vec<String> {
    let (sql, parameters) = build_sql(plan, split).await;
    let mut statement = database.prepare(&sql).unwrap();
    let columns = statement.column_count();
    let binds: Vec<SqliteValue> = parameters.iter().map(sqlite_value).collect();
    let mut rows = statement
        .query_map(rusqlite::params_from_iter(binds), |row| {
            (0..columns)
                .map(|column| {
                    row.get::<_, SqliteValue>(column).map(|value| match value {
                        SqliteValue::Integer(v) => v.to_string(),
                        SqliteValue::Text(v) => v,
                        other => format!("{other:?}"),
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|cells| cells.join("|"))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows.sort();
    rows
}

#[tokio::test]
async fn test_child_partition_splits_return_every_row_once_on_query_plan_path() {
    let database = load_tables();
    for (query, plan) in PLANS {
        let expected = run(&database, Some(plan), &split("*", "*")).await;
        let mut union = Vec::new();
        for (child, _, _) in CHILDREN {
            union.extend(run(&database, Some(plan), &split("test_schema", child)).await);
        }
        assert_eq!(
            union.len(),
            union.iter().collect::<HashSet<_>>().len(),
            "duplicate rows for {query}: {union:?}"
        );
        union.sort();
        assert_eq!(expected, union, "rows for {query}");
    }
}

#[tokio::test]
async fn test_child_partition_splits_match_non_plan_path() {
    let database = load_tables();
    for (child, low, high) in CHILDREN {
        let with_plan = run(&database, Some(PLANS[0].1), &split("test_schema", child)).await;
        let without_plan = run(&database, None, &split("test_schema", child)).await;
        assert_eq!(
            usize::try_from(high - low + 1).unwrap(),
            with_plan.len(),
            "rows for {child}"
        );
        assert_eq!(without_plan, with_plan, "rows for {child}");
    }
}

#[tokio::test]
async fn test_child_partition_split_sql_on_query_plan_path() {
    let plan = PLANS
        .iter()
        .find(|(query, _)| *query == OR_FILTER)
        .unwrap()
        .1;
    let (sql, _) = build_sql(Some(plan), &split("test_schema", "test_table_low")).await;
    assert_eq!(
        "SELECT *\nFROM (SELECT * FROM \"test_schema\".\"test_table_low\") AS \"test_table\"\nWHERE \"id\" < ? OR \"name\" = ?",
        sql
    );
}
