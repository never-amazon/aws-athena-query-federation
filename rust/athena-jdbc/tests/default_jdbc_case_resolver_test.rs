mod jdbc_support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::resolver::{
    CaseResolver, FederationSdkCasingMode, CASING_MODE_CONFIGURATION_KEY, DEFAULT_GLUE_CONNECTION,
};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::resolver::{
    default_do_get_schema_name_case_insensitively, default_do_get_table_name_case_insensitively,
    DefaultJdbcCaseResolver, JdbcCaseResolver, JdbcCaseResolverExt,
};
use athena_jdbc::sql::{JdbcConnection, SqlException, SqlValue};
use jdbc_support::{FakeConnection, ResultSetData};

const TEST_SCHEMA_NAME: &str = "oRaNgE";
const TEST_TABLE_NAME: &str = "ApPlE";
const TEST_SCHEMA_UPPER: &str = "ORANGE";
const TEST_TABLE_UPPER: &str = "APPLE";
const TEST_SCHEMA_LOWER: &str = "orange";
const TEST_TABLE_LOWER: &str = "apple";
const TEST_SOURCE_TYPE: &str = "asdf";
const TEST_SAPHANA_SOURCE: &str = "saphana";
const TEST_TABLE_ANNOTATION: &str = "ApPlE@schemaCase=upper&tableCase=lower";
const TEST_INPUT_SCHEMA: &str = "MySchema";
const TEST_EXPECTED_SCHEMA: &str = "myschema";
const TEST_INPUT_TABLE: &str = "Table1";
const TEST_EXPECTED_TABLE: &str = "table1";
const TEST_INVALID_SCHEMA: &str = "InvalidSchema";
const TEST_INVALID_TABLE: &str = "InvalidTable";

struct TestableHelper {
    base: CaseResolver,
}

impl TestableHelper {
    fn new(source_type: &str) -> TestableHelper {
        TestableHelper {
            base: CaseResolver::new(
                source_type,
                FederationSdkCasingMode::Upper,
                FederationSdkCasingMode::Lower,
            ),
        }
    }
}

#[async_trait]
impl JdbcCaseResolver for TestableHelper {
    fn case_resolver(&self) -> &CaseResolver {
        &self.base
    }

    async fn do_get_schema_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        default_do_get_schema_name_case_insensitively(self, connection, schema_name_input).await
    }

    async fn do_get_table_name_case_insensitively(
        &self,
        connection: &dyn JdbcConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        default_do_get_table_name_case_insensitively(
            self,
            connection,
            schema_name_in_correct_case,
            table_name_input,
        )
        .await
    }

    fn get_case_insensitively_schema_name_query_template(&self) -> Result<String> {
        Ok(
            "SELECT schema_name FROM information_schema.schemata WHERE lower(schema_name) = ?"
                .to_string(),
        )
    }

    fn get_case_insensitively_schema_name_column_key(&self) -> Result<String> {
        Ok("schema_name".to_string())
    }

    fn get_case_insensitively_table_name_query_template(&self) -> Result<Vec<String>> {
        Ok(vec![
            "SELECT table_name FROM information_schema.tables WHERE table_schema = ? AND lower(table_name) = ?"
                .to_string(),
        ])
    }

    fn get_case_insensitively_table_name_column_key(&self) -> Result<String> {
        Ok("table_name".to_string())
    }
}

fn resolver(source_type: &str) -> DefaultJdbcCaseResolver {
    DefaultJdbcCaseResolver::new_with_casing_modes(
        source_type,
        FederationSdkCasingMode::Upper,
        FederationSdkCasingMode::Lower,
    )
}

fn mode(mode: FederationSdkCasingMode) -> HashMap<String, String> {
    HashMap::from([(
        CASING_MODE_CONFIGURATION_KEY.to_string(),
        mode.as_str().to_string(),
    )])
}

fn glue() -> HashMap<String, String> {
    HashMap::from([(
        DEFAULT_GLUE_CONNECTION.to_string(),
        TEST_SOURCE_TYPE.to_string(),
    )])
}

fn assert_unsupported<T: std::fmt::Debug>(result: &Result<T>) {
    assert!(
        matches!(result, Err(FederationError::UnsupportedOperation(_))),
        "{result:?}"
    );
}

fn counting_query(connection: &FakeConnection, column: &'static str, value: &'static str) {
    let calls = Arc::new(AtomicUsize::new(0));
    connection.on_query(move |_| {
        let rows = if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            1
        } else {
            2
        };
        Ok(ResultSetData::of_strings(
            &[column],
            &vec![&[value][..]; rows],
        ))
    });
}

fn parameter_count(connection: &FakeConnection) -> usize {
    connection
        .executed()
        .iter()
        .map(|s| {
            s.parameters()
                .iter()
                .filter(|p| matches!(p, SqlValue::String(_)))
                .count()
        })
        .sum()
}

#[tokio::test]
async fn test_default_jdbc_case_resolver_default_casing() {
    let test = resolver(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    let none = HashMap::new();

    let name = test
        .get_adjusted_schema_name_string(&connection, TEST_TABLE_NAME, &none)
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_UPPER, name);
    let name = test
        .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &none)
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_UPPER, name);
    let object = test
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
            &none,
        )
        .await
        .unwrap();
    assert_eq!(TableName::new(TEST_SCHEMA_UPPER, TEST_TABLE_UPPER), object);

    let name = test
        .get_adjusted_schema_name_string(&connection, TEST_TABLE_NAME, &glue())
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_LOWER, name);
    let name = test
        .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &glue())
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_LOWER, name);
    let object = test
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
            &glue(),
        )
        .await
        .unwrap();
    assert_eq!(TableName::new(TEST_SCHEMA_LOWER, TEST_TABLE_LOWER), object);
}

#[tokio::test]
async fn test_none_override_case() {
    let test = resolver(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    let config = mode(FederationSdkCasingMode::None);
    let schema = test
        .get_adjusted_schema_name_string(&connection, TEST_SCHEMA_NAME, &config)
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_NAME, schema);
    let table = test
        .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &config)
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_NAME, table);
    let object = test
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
            &config,
        )
        .await
        .unwrap();
    assert_eq!(TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME), object);
}

#[tokio::test]
async fn test_annotation_override_case() {
    let test = resolver(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    let config = mode(FederationSdkCasingMode::Annotation);
    assert_unsupported(
        &test
            .get_adjusted_schema_name_string(&connection, TEST_SCHEMA_NAME, &config)
            .await,
    );
    assert_unsupported(
        &test
            .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &config)
            .await,
    );
    assert_unsupported(
        &test
            .get_adjusted_table_name_object(
                &connection,
                &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
                &config,
            )
            .await,
    );

    let saphana = resolver(TEST_SAPHANA_SOURCE);
    assert_unsupported(
        &saphana
            .get_adjusted_schema_name_string(&connection, TEST_SCHEMA_NAME, &config)
            .await,
    );
    assert_unsupported(
        &saphana
            .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &config)
            .await,
    );
    let adjusted = saphana
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
            &config,
        )
        .await
        .unwrap();
    assert_eq!(TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME), adjusted);
    let adjusted = saphana
        .get_adjusted_table_name_object(
            &connection,
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_ANNOTATION),
            &config,
        )
        .await
        .unwrap();
    assert_eq!(
        TableName::new(TEST_SCHEMA_UPPER, TEST_TABLE_LOWER),
        adjusted
    );
}

#[tokio::test]
async fn test_case_insensitively_override_case() {
    let test = resolver(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    let config = mode(FederationSdkCasingMode::CaseInsensitiveSearch);
    assert_unsupported(
        &test
            .get_adjusted_schema_name_string(&connection, TEST_SCHEMA_NAME, &config)
            .await,
    );
    assert_unsupported(
        &test
            .get_adjusted_table_name_string(&connection, TEST_SCHEMA_NAME, TEST_TABLE_NAME, &config)
            .await,
    );
    assert_unsupported(
        &test
            .get_adjusted_table_name_object(
                &connection,
                &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
                &config,
            )
            .await,
    );
}

#[tokio::test]
async fn test_schema_name_case_insensitively() {
    let helper = TestableHelper::new(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    counting_query(&connection, "schema_name", TEST_EXPECTED_SCHEMA);
    let config = mode(FederationSdkCasingMode::CaseInsensitiveSearch);
    let result = helper
        .get_adjusted_schema_name_string(&connection, TEST_INPUT_SCHEMA, &config)
        .await
        .unwrap();
    assert_eq!(TEST_EXPECTED_SCHEMA, result);

    let error = helper
        .get_adjusted_schema_name_string(&connection, TEST_INPUT_SCHEMA, &config)
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::AthenaConnector(_)));
    assert!(error
        .to_string()
        .contains("Schema name case insensitive match failed"));

    assert_eq!(2, parameter_count(&connection));
    assert_eq!(2, connection.prepared_sql().len());
}

#[tokio::test]
async fn test_do_get_schema_name_case_insensitively_sql_exception_thrown() {
    let helper = TestableHelper::new(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlException::new("DB error")));
    let error = helper
        .do_get_schema_name_case_insensitively(&connection, TEST_INVALID_SCHEMA, &HashMap::new())
        .await
        .unwrap_err();
    assert!(error.is_runtime_exception());
    assert!(error
        .to_string()
        .contains("getSchemaNameCaseInsensitively query failed for"));
}

#[tokio::test]
async fn test_table_name_case_insensitively() {
    let helper = TestableHelper::new(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    counting_query(&connection, "table_name", TEST_EXPECTED_TABLE);
    let config = mode(FederationSdkCasingMode::CaseInsensitiveSearch);
    let result = helper
        .get_adjusted_table_name_string(&connection, TEST_INPUT_SCHEMA, TEST_INPUT_TABLE, &config)
        .await
        .unwrap();
    assert_eq!(TEST_EXPECTED_TABLE, result);

    let error = helper
        .get_adjusted_table_name_string(&connection, TEST_INPUT_SCHEMA, TEST_INPUT_TABLE, &config)
        .await
        .unwrap_err();
    assert!(matches!(error, FederationError::AthenaConnector(_)));
    assert!(error
        .to_string()
        .contains("Table name case insensitive match failed"));

    assert_eq!(4, parameter_count(&connection));
    assert_eq!(2, connection.prepared_sql().len());
}

#[tokio::test]
async fn test_do_get_table_name_case_insensitively_sql_exception_thrown() {
    let helper = TestableHelper::new(TEST_SOURCE_TYPE);
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlException::new("DB error")));
    let error = helper
        .do_get_table_name_case_insensitively(
            &connection,
            TEST_INVALID_SCHEMA,
            TEST_INVALID_TABLE,
            &HashMap::new(),
        )
        .await
        .unwrap_err();
    assert!(error.is_runtime_exception());
    assert!(error
        .to_string()
        .contains("getTableNameCaseInsensitively query failed for schema"));
}
