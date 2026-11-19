mod jdbc_support;

use std::collections::HashMap;

use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::FederationSdkCasingMode;
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::resolver::{CaseLookup, JdbcCaseResolver};
use athena_jdbc::sql::{PreparedStatement, SqlValue};
use jdbc_support::{FakeConnection, ResultSetData};

const CASING_MODE_CONFIG_KEY: &str = "casing_mode";
const CASING_MODE_LOWER: &str = "LOWER";
const CASING_MODE_UPPER: &str = "UPPER";
const CASING_MODE_CASE_INSENSITIVE_SEARCH: &str = "CASE_INSENSITIVE_SEARCH";
const CASING_MODE_NONE: &str = "NONE";
const CASING_MODE_ANNOTATION: &str = "ANNOTATION";
const TEST_SCHEMA_NAME: &str = "TestSchema";
const TEST_TABLE_NAME: &str = "TestTable";
const TEST_SCHEMA_LOWER: &str = "testschema";
const TEST_TABLE_LOWER: &str = "testtable";
const TEST_SCHEMA_UPPER: &str = "TESTSCHEMA";
const TEST_TABLE_UPPER: &str = "TESTTABLE";
const DB_TYPE_TESTDB: &str = "testdb";
const DB_TYPE_MYSQL: &str = "mysql";
const DB_TYPE_UNSUPPORTED: &str = "unsupportedtype";

#[derive(Clone, Copy, PartialEq)]
enum Matches {
    Single,
    Multiple,
}

const SCHEMA_QUERY: &str = "schema lookup";
const TABLE_QUERY: &str = "table lookup";
const TEST_LOOKUP: CaseLookup = CaseLookup {
    schema_query: SCHEMA_QUERY,
    schema_column: "schema_name",
    table_queries: &[TABLE_QUERY],
    table_column: "table_name",
};

struct TestJdbcCaseResolver {
    inner: JdbcCaseResolver,
    schema_matches: Matches,
    table_matches: Matches,
}

fn parameter(statement: &PreparedStatement, index: usize) -> String {
    match statement.parameter(index) {
        Some(SqlValue::String(value)) => value.clone(),
        other => panic!("unexpected parameter {other:?}"),
    }
}

fn rows(column: &str, values: &[&str]) -> ResultSetData {
    let rows: Vec<&[&str]> = values.iter().map(std::slice::from_ref).collect();
    ResultSetData::of_strings(&[column], &rows)
}

impl TestJdbcCaseResolver {
    fn new(source_type: &str) -> TestJdbcCaseResolver {
        TestJdbcCaseResolver::new_with_casing_modes(
            source_type,
            FederationSdkCasingMode::None,
            FederationSdkCasingMode::None,
        )
    }

    fn new_with_casing_modes(
        source_type: &str,
        non_glue: FederationSdkCasingMode,
        glue: FederationSdkCasingMode,
    ) -> TestJdbcCaseResolver {
        TestJdbcCaseResolver {
            inner: JdbcCaseResolver::new(source_type, non_glue, glue).with_lookup(TEST_LOOKUP),
            schema_matches: Matches::Single,
            table_matches: Matches::Single,
        }
    }

    fn source_type(&self) -> &str {
        self.inner.source_type()
    }

    fn script(&self, connection: &FakeConnection) {
        let schema_matches = self.schema_matches;
        let table_matches = self.table_matches;
        connection.on_query(move |statement| {
            if statement.sql() == SCHEMA_QUERY {
                if schema_matches == Matches::Multiple {
                    return Ok(rows("schema_name", &["Schema1", "Schema2"]));
                }
                if parameter(statement, 1) == TEST_SCHEMA_LOWER {
                    return Ok(rows("schema_name", &[TEST_SCHEMA_NAME]));
                }
                return Ok(rows("schema_name", &[]));
            }
            if table_matches == Matches::Multiple {
                return Ok(rows("table_name", &["Table1", "Table2"]));
            }
            if parameter(statement, 2) == TEST_TABLE_LOWER {
                return Ok(rows("table_name", &[TEST_TABLE_NAME]));
            }
            Ok(rows("table_name", &[]))
        });
    }

    async fn get_adjusted_schema_name_string(
        &self,
        connection: &FakeConnection,
        schema_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String> {
        self.script(connection);
        self.inner
            .get_adjusted_schema_name_string(connection, schema_name_input, config_options)
            .await
    }

    async fn get_adjusted_table_name_string(
        &self,
        connection: &FakeConnection,
        schema_name_in_correct_case: &str,
        table_name_input: &str,
        config_options: &HashMap<String, String>,
    ) -> Result<String> {
        self.script(connection);
        self.inner
            .get_adjusted_table_name_string(
                connection,
                schema_name_in_correct_case,
                table_name_input,
                config_options,
            )
            .await
    }

    async fn get_adjusted_table_name_object(
        &self,
        connection: &FakeConnection,
        table_name_object: &TableName,
        config_options: &HashMap<String, String>,
    ) -> Result<TableName> {
        self.script(connection);
        self.inner
            .get_adjusted_table_name_object(connection, table_name_object, config_options)
            .await
    }
}

fn plain_resolver() -> JdbcCaseResolver {
    JdbcCaseResolver::new(
        DB_TYPE_UNSUPPORTED,
        FederationSdkCasingMode::None,
        FederationSdkCasingMode::None,
    )
}

fn config(mode: &str) -> HashMap<String, String> {
    HashMap::from([(CASING_MODE_CONFIG_KEY.to_string(), mode.to_string())])
}

fn assert_unsupported<T: std::fmt::Debug>(result: &Result<T>) {
    assert!(
        matches!(result, Err(FederationError::UnsupportedOperation(_))),
        "{result:?}"
    );
}

fn assert_connector<T: std::fmt::Debug>(result: &Result<T>) {
    assert!(
        matches!(result, Err(FederationError::AthenaConnector(_))),
        "{result:?}"
    );
}

#[test]
fn test_constructor_with_source_type() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_MYSQL);
    assert_eq!(DB_TYPE_MYSQL, resolver.source_type());
}

#[test]
fn test_constructor_with_casing_modes() {
    let resolver = TestJdbcCaseResolver::new_with_casing_modes(
        DB_TYPE_MYSQL,
        FederationSdkCasingMode::Lower,
        FederationSdkCasingMode::Upper,
    );
    assert_eq!(DB_TYPE_MYSQL, resolver.source_type());
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_lower_case() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_schema_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            &config(CASING_MODE_LOWER),
        )
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_LOWER, result);
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_upper_case() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_schema_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_LOWER,
            &config(CASING_MODE_UPPER),
        )
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_UPPER, result);
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_case_insensitive() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_schema_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_LOWER,
            &config(CASING_MODE_CASE_INSENSITIVE_SEARCH),
        )
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_NAME, result);
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_none() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_schema_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            &config(CASING_MODE_NONE),
        )
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_NAME, result);
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_no_casing_mode() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_schema_name_string(&FakeConnection::new(), TEST_SCHEMA_NAME, &HashMap::new())
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_NAME, result);
}

#[tokio::test]
async fn test_get_adjusted_schema_name_string_annotation_mode() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    assert_unsupported(
        &resolver
            .get_adjusted_schema_name_string(
                &FakeConnection::new(),
                TEST_SCHEMA_NAME,
                &config(CASING_MODE_ANNOTATION),
            )
            .await,
    );
}

#[tokio::test]
async fn test_get_adjusted_table_name_string_lower_case() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_table_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            TEST_TABLE_NAME,
            &config(CASING_MODE_LOWER),
        )
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_LOWER, result);
}

#[tokio::test]
async fn test_get_adjusted_table_name_string_upper_case() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_table_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            TEST_TABLE_LOWER,
            &config(CASING_MODE_UPPER),
        )
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_UPPER, result);
}

#[tokio::test]
async fn test_get_adjusted_table_name_string_case_insensitive() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_table_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            TEST_TABLE_LOWER,
            &config(CASING_MODE_CASE_INSENSITIVE_SEARCH),
        )
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_NAME, result);
}

#[tokio::test]
async fn test_get_adjusted_table_name_string_none() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_table_name_string(
            &FakeConnection::new(),
            TEST_SCHEMA_NAME,
            TEST_TABLE_NAME,
            &config(CASING_MODE_NONE),
        )
        .await
        .unwrap();
    assert_eq!(TEST_TABLE_NAME, result);
}

#[tokio::test]
async fn test_get_adjusted_table_name_string_annotation_mode() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    assert_unsupported(
        &resolver
            .get_adjusted_table_name_string(
                &FakeConnection::new(),
                TEST_SCHEMA_NAME,
                TEST_TABLE_NAME,
                &config(CASING_MODE_ANNOTATION),
            )
            .await,
    );
}

#[tokio::test]
async fn test_get_adjusted_table_name_object_lower_case() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    let result = resolver
        .get_adjusted_table_name_object(
            &FakeConnection::new(),
            &TableName::new(TEST_SCHEMA_NAME, TEST_TABLE_NAME),
            &config(CASING_MODE_LOWER),
        )
        .await
        .unwrap();
    assert_eq!(TEST_SCHEMA_LOWER, result.schema_name());
    assert_eq!(TEST_TABLE_LOWER, result.table_name());
}

#[tokio::test]
async fn test_get_adjusted_table_name_object_annotation_mode_unsupported_type() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    assert_unsupported(
        &resolver
            .get_adjusted_table_name_object(
                &FakeConnection::new(),
                &TableName::new(TEST_SCHEMA_NAME, "TestTable@table=lower"),
                &config(CASING_MODE_ANNOTATION),
            )
            .await,
    );
}

#[tokio::test]
async fn test_schema_name_case_insensitively_multiple_matches() {
    let mut resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    resolver.schema_matches = Matches::Multiple;
    assert_connector(
        &resolver
            .get_adjusted_schema_name_string(
                &FakeConnection::new(),
                "schema",
                &config(CASING_MODE_CASE_INSENSITIVE_SEARCH),
            )
            .await,
    );
}

#[tokio::test]
async fn test_table_name_case_insensitively_multiple_matches() {
    let mut resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    resolver.table_matches = Matches::Multiple;
    assert_connector(
        &resolver
            .get_adjusted_table_name_string(
                &FakeConnection::new(),
                TEST_SCHEMA_NAME,
                "table",
                &config(CASING_MODE_CASE_INSENSITIVE_SEARCH),
            )
            .await,
    );
}

#[tokio::test]
async fn test_get_case_insensitively_schema_name_query_template() {
    let resolver = plain_resolver();
    let result = resolver
        .schema_name_case_insensitively(&FakeConnection::new(), "test")
        .await;
    assert_unsupported(&result);
    assert_eq!(
        "CASE_INSENSITIVE_SEARCH is not supported for type: 'unsupportedtype', please provide SchemaNameQuery",
        result.unwrap_err().to_string()
    );
}

#[tokio::test]
async fn test_get_case_insensitively_schema_name_column_key() {
    let resolver = plain_resolver();
    assert!(resolver.lookup().is_none());
    assert_unsupported(
        &resolver
            .schema_name_case_insensitively(&FakeConnection::new(), "test")
            .await,
    );
}

#[tokio::test]
async fn test_do_get_schema_name_case_insensitively_default_implementation() {
    let resolver = plain_resolver();
    assert_unsupported(
        &resolver
            .schema_name_case_insensitively(&FakeConnection::new(), "test")
            .await,
    );
}

#[tokio::test]
async fn test_do_get_table_name_case_insensitively_default_implementation() {
    let resolver = plain_resolver();
    assert_unsupported(
        &resolver
            .table_name_case_insensitively(&FakeConnection::new(), TEST_SCHEMA_NAME, "test")
            .await,
    );
}
