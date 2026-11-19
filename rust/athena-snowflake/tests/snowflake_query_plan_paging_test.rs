mod snowflake_support;

use std::collections::{HashMap, HashSet};

use arrow::datatypes::{DataType, Field, Schema};
use indexmap::IndexMap;
use rusqlite::types::Value as SqliteValue;
use rusqlite::Connection;

use athena_federation_sdk::domain::predicate::{Constraints, QueryPlan, DEFAULT_NO_LIMIT};
use athena_federation_sdk::domain::{S3SpillLocation, SpillLocation, Split, TableName};
use athena_jdbc::query::SplitQuery;
use athena_jdbc::sql::SqlValue;
use athena_snowflake::{constants, query_string_builder, SnowflakeQueryStringBuilder};

use snowflake_support::FakeConnection;

const TOP_N: &str = "SELECT id, name FROM test_table ORDER BY name DESC LIMIT 7";
const TOP_N_LIMIT: usize = 7;
const OR_FILTER: &str = "SELECT id, name FROM test_table WHERE id < 5 OR name = 'n17'";
const ROW_COUNTS: [usize; 3] = [1, 7, 23];
const PAGE_SIZES: [usize; 5] = [1, 3, 7, 23, 50];

const PLANS: [(&str, &str); 5] = [
    (
        "SELECT id, name FROM test_table",
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
        TOP_N,
        "GpQBEpEBCoQBGoEBCgIKABJ3KnUKAgoAEl86XQoGEgQKAgIDEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGggSBgoCEgAiABoKEggKBBICCAEiABoOCgoSCAoEEgIIASIAEAMYACAHEgJJRBIETkFNRTILEEoqB2lzdGhtdXM=",
    ),
    (
        "SELECT name FROM test_table WHERE id >= 3",
        "Ch4IARIaL2Z1bmN0aW9uc19jb21wYXJpc29uLnlhbWwSExoRCAEQARoLZ3RlOmFueV9hbnkahwEShAEKfDp6CgUSAwoBAhJlEmMKAgoAEj0KOwoCCgASGgoCaWQKBG5hbWUSDgoEKgIQAgoEYgIQARgCOhkKC3Rlc3Rfc2NoZW1hCgp0ZXN0X3RhYmxlGh4aHAgBGgQKAhACIgoaCBIGCgISACIAIgYaBAoCKAMaChIICgQSAggBIgASBE5BTUUyCxBKKgdpc3RobXVz",
    ),
];

fn query_builder() -> SnowflakeQueryStringBuilder {
    query_string_builder()
}

fn schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Int32, true),
        Field::new("name", DataType::Utf8, true),
    ])
}

fn constraints(plan: &str) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        Vec::new(),
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        Some(QueryPlan::new(Some(String::new()), plan)),
    )
}

fn spill_location() -> SpillLocation {
    SpillLocation::S3(
        S3SpillLocation::new_builder()
            .with_bucket("bucket")
            .with_prefix("prefix")
            .build(),
    )
}

fn unpaged_split() -> Split {
    Split::new_builder(Some(spill_location()), None).build()
}

fn paged_split(page_size: usize, offset: usize) -> Split {
    Split::new_builder(Some(spill_location()), None)
        .add(
            constants::BLOCK_PARTITION_COLUMN_NAME,
            format!("partition-primary-\"id\"-limit-{page_size}-offset-{offset}"),
        )
        .build()
}

fn load_table(row_count: usize) -> Connection {
    let database = Connection::open_in_memory().unwrap();
    database
        .execute_batch(
            "ATTACH DATABASE ':memory:' AS \"test_schema\";
             CREATE TABLE \"test_schema\".\"test_table\" (\"id\" INTEGER PRIMARY KEY, \"name\" TEXT);",
        )
        .unwrap();
    for id in (1..=row_count).rev() {
        database
            .execute(
                "INSERT INTO \"test_schema\".\"test_table\" VALUES (?1, ?2)",
                rusqlite::params![i64::try_from(id).unwrap(), format!("n{id}")],
            )
            .unwrap();
    }
    database
}

async fn build_sql(plan: &str, split: &Split) -> (String, Vec<SqlValue>) {
    let connection = FakeConnection::new();
    let statement = query_builder()
        .build_sql(
            &connection,
            &SplitQuery {
                catalog_name: "test_catalog",
                table_name: &TableName::new("test_schema", "test_table"),
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

async fn run(database: &Connection, plan: &str, split: &Split) -> Vec<String> {
    let (sql, parameters) = build_sql(plan, split).await;
    let mut statement = database.prepare(&sql).unwrap();
    let columns = statement.column_count();
    let binds: Vec<SqliteValue> = parameters.iter().map(sqlite_value).collect();
    statement
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
        .unwrap()
}

fn sorted(rows: &[String], query: &str) -> Vec<String> {
    let mut copy = rows.to_vec();
    if query == TOP_N {
        copy.sort_by(|a, b| {
            let name = |row: &str| row[row.find('|').unwrap() + 1..].to_string();
            name(b).cmp(&name(a))
        });
        copy.truncate(TOP_N_LIMIT);
    }
    copy.sort();
    copy
}

#[tokio::test]
async fn test_paged_splits_return_every_row_once_on_query_plan_path() {
    let mut configurations = 0;
    for row_count in ROW_COUNTS {
        let database = load_table(row_count);
        for (query, plan) in PLANS {
            let expected = run(&database, plan, &unpaged_split()).await;
            for page_size in PAGE_SIZES {
                let mut union = Vec::new();
                for offset in (0..row_count).step_by(page_size) {
                    union.extend(run(&database, plan, &paged_split(page_size, offset)).await);
                }
                let config = format!("{query} rows={row_count} page={page_size}");
                assert_eq!(
                    union.len(),
                    union.iter().collect::<HashSet<_>>().len(),
                    "duplicate rows for {config}: {union:?}"
                );
                assert_eq!(
                    sorted(&expected, query),
                    sorted(&union, query),
                    "rows for {config}"
                );
                configurations += 1;
            }
        }
    }
    assert_eq!(
        ROW_COUNTS.len() * PLANS.len() * PAGE_SIZES.len(),
        configurations
    );
}

#[tokio::test]
async fn test_paged_split_sql_on_query_plan_path() {
    let plan = PLANS
        .iter()
        .find(|(query, _)| *query == OR_FILTER)
        .unwrap()
        .1;
    let (sql, _) = build_sql(plan, &paged_split(2, 4)).await;
    assert_eq!(
        "SELECT *\nFROM (SELECT * FROM \"test_schema\".\"test_table\" ORDER BY \"id\" LIMIT 2 OFFSET 4) AS \"test_table\"\nWHERE \"id\" < ? OR \"name\" = ?",
        sql
    );
}

#[tokio::test]
async fn test_paged_split_rejects_query_plan_without_table() {
    let plan = "GlkSVwpFCkMKAgoAEiMKBkVYUFIkMAoGRVhQUiQxEhEKBCoCEAIKB6oBBAgBGAIYAioYCgoKAigBCgSqAQFhCgoKAigCCgSqAQFiEgZFWFBSJDASBkVYUFIkMTILEEoqB2lzdGhtdXM=";
    let connection = FakeConnection::new();
    let error = query_builder()
        .build_sql(
            &connection,
            &SplitQuery {
                catalog_name: "test_catalog",
                table_name: &TableName::new("test_schema", "test_table"),
                schema: &schema(),
                constraints: &constraints(plan),
                split: &paged_split(2, 0),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        "Failed to prepare statement with Calcite",
        error.to_string()
    );
    assert_eq!(
        "Cannot apply the split to a query plan that does not read one table: SELECT *\nFROM (VALUES (1, 'a'),\n(2, 'b')) AS \"t\" (\"EXPR$0\", \"EXPR$1\")",
        std::error::Error::source(&error)
            .unwrap()
            .to_string()
    );
}
