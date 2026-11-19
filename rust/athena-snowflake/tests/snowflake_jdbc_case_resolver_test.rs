mod snowflake_support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::{FederationSdkCasingMode, CASING_MODE_CONFIGURATION_KEY};
use athena_federation_sdk::FederationError;
use athena_jdbc::resolver::JdbcCaseResolverExt;
use athena_jdbc::sql::SqlValue;
use athena_snowflake::resolver::SnowflakeJdbcCaseResolver;
use athena_snowflake::SnowflakeConstants;
use snowflake_support::{FakeConnection, ResultSetData};

const SCHEMA_QUERY: &str = "select * from INFORMATION_SCHEMA.SCHEMATA where lower(SCHEMA_NAME) = ?";
const TABLE_QUERY: &str =
    "select * from INFORMATION_SCHEMA.TABLES where TABLE_SCHEMA = ? and lower(TABLE_NAME) = ?";

fn casing(mode: FederationSdkCasingMode) -> HashMap<String, String> {
    HashMap::from([(
        CASING_MODE_CONFIGURATION_KEY.to_string(),
        mode.as_str().to_string(),
    )])
}

fn strings(values: &[SqlValue]) -> Vec<String> {
    values
        .iter()
        .map(|value| match value {
            SqlValue::String(text) => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn test_annotation_override_case() {
    let schema_name = "oRaNgE";
    let table_name = "ApPlE";
    let snowflake = SnowflakeJdbcCaseResolver::new(SnowflakeConstants::SNOWFLAKE_NAME);
    let connection = FakeConnection::new();
    let annotation = casing(FederationSdkCasingMode::Annotation);

    let error = snowflake
        .get_adjusted_schema_name_string(&connection, schema_name, &annotation)
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::UnsupportedOperation(_)));
    let error = snowflake
        .get_adjusted_table_name_string(&connection, schema_name, table_name, &annotation)
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::UnsupportedOperation(_)));

    let adjusted = snowflake
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(schema_name, table_name),
            &annotation,
        )
        .await
        .unwrap();
    assert_eq!(
        TableName::new(schema_name.to_uppercase(), table_name.to_uppercase()),
        adjusted
    );

    let table_name_annotation = "ApPlE@schemaCase=upper&tableCase=lower";
    let adjusted = snowflake
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(schema_name, table_name_annotation),
            &annotation,
        )
        .await
        .unwrap();
    assert_eq!(
        TableName::new(schema_name.to_uppercase(), table_name.to_lowercase()),
        adjusted
    );
    assert!(connection.executed().is_empty());
}

#[tokio::test]
async fn test_case_insensitive_case_on_name() {
    let schema_name = "oRaNgE";
    let table_name = "ApPlE";
    let snowflake = SnowflakeJdbcCaseResolver::new(SnowflakeConstants::SNOWFLAKE_NAME);
    let connection = FakeConnection::new();
    let search = casing(FederationSdkCasingMode::CaseInsensitiveSearch);

    let schema_lower = schema_name.to_lowercase();
    connection.on_query(move |_| {
        Ok(ResultSetData::of_strings(
            &["SCHEMA_NAME"],
            &[&[schema_lower.as_str()]],
        ))
    });
    let adjusted_schema = snowflake
        .get_adjusted_schema_name_string(&connection, schema_name, &search)
        .await
        .unwrap();
    assert_eq!(schema_name.to_lowercase(), adjusted_schema);

    let table_upper = table_name.to_uppercase();
    connection.on_query(move |_| {
        Ok(ResultSetData::of_strings(
            &["TABLE_NAME"],
            &[&[table_upper.as_str()]],
        ))
    });
    let adjusted_table = snowflake
        .get_adjusted_table_name_string(&connection, schema_name, table_name, &search)
        .await
        .unwrap();
    assert_eq!(table_name.to_uppercase(), adjusted_table);

    assert_eq!(vec![SCHEMA_QUERY, TABLE_QUERY], connection.prepared_sql());
    let executed = connection.executed();
    assert_eq!(vec!["orange"], strings(executed[0].parameters()));
    assert_eq!(vec!["oRaNgE", "apple"], strings(executed[1].parameters()));
}

#[tokio::test]
async fn test_case_insensitive_case_on_object() {
    let schema_name = "oRaNgE";
    let table_name = "ApPlE";
    let snowflake = SnowflakeJdbcCaseResolver::new(SnowflakeConstants::SNOWFLAKE_NAME);
    let connection = FakeConnection::new();
    let search = casing(FederationSdkCasingMode::CaseInsensitiveSearch);

    let calls = AtomicUsize::new(0);
    let schema_lower = schema_name.to_lowercase();
    let table_upper = table_name.to_uppercase();
    connection.on_query(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(ResultSetData::of_strings(
                &["SCHEMA_NAME"],
                &[&[schema_lower.as_str()]],
            ))
        } else {
            Ok(ResultSetData::of_strings(
                &["TABLE_NAME"],
                &[&[table_upper.as_str()]],
            ))
        }
    });

    let adjusted = snowflake
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(schema_name, table_name),
            &search,
        )
        .await
        .unwrap();

    assert_eq!(
        TableName::new(schema_name.to_lowercase(), table_name.to_uppercase()),
        adjusted
    );
    let executed = connection.executed();
    assert_eq!(2, executed.len());
    assert_eq!(vec!["orange", "apple"], strings(executed[1].parameters()));
}
