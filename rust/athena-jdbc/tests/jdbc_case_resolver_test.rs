mod jdbc_support;

use std::collections::HashMap;

use async_trait::async_trait;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::resolver::{CaseResolver, FederationSdkCasingMode};
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::resolver::{JdbcCaseResolver, JdbcCaseResolverExt};
use athena_jdbc::sql::JdbcConnection;
use jdbc_support::FakeConnection;

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

struct TestJdbcCaseResolver {
    base: CaseResolver,
    schema_matches: Matches,
    table_matches: Matches,
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
            base: CaseResolver::new(source_type, non_glue, glue),
            schema_matches: Matches::Single,
            table_matches: Matches::Single,
        }
    }
}

#[async_trait]
impl JdbcCaseResolver for TestJdbcCaseResolver {
    fn case_resolver(&self) -> &CaseResolver {
        &self.base
    }

    async fn do_get_schema_name_case_insensitively(
        &self,
        _connection: &dyn JdbcConnection,
        schema_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        if self.schema_matches == Matches::Multiple {
            return Ok(vec!["Schema1".to_string(), "Schema2".to_string()]);
        }
        if TEST_SCHEMA_LOWER.eq_ignore_ascii_case(schema_name_input) {
            return Ok(vec![TEST_SCHEMA_NAME.to_string()]);
        }
        Ok(Vec::new())
    }

    async fn do_get_table_name_case_insensitively(
        &self,
        _connection: &dyn JdbcConnection,
        _schema_name_in_correct_case: &str,
        table_name_input: &str,
        _config_options: &HashMap<String, String>,
    ) -> Result<Vec<String>> {
        if self.table_matches == Matches::Multiple {
            return Ok(vec!["Table1".to_string(), "Table2".to_string()]);
        }
        if TEST_TABLE_LOWER.eq_ignore_ascii_case(table_name_input) {
            return Ok(vec![TEST_TABLE_NAME.to_string()]);
        }
        Ok(Vec::new())
    }
}

struct PlainJdbcCaseResolver {
    base: CaseResolver,
}

impl JdbcCaseResolver for PlainJdbcCaseResolver {
    fn case_resolver(&self) -> &CaseResolver {
        &self.base
    }
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

#[test]
fn test_get_case_insensitively_schema_name_query_template() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    assert_unsupported(&resolver.get_case_insensitively_schema_name_query_template());
}

#[test]
fn test_get_case_insensitively_schema_name_column_key() {
    let resolver = TestJdbcCaseResolver::new(DB_TYPE_TESTDB);
    assert_unsupported(&resolver.get_case_insensitively_schema_name_column_key());
}

#[tokio::test]
async fn test_do_get_schema_name_case_insensitively_default_implementation() {
    let resolver = PlainJdbcCaseResolver {
        base: CaseResolver::new(
            DB_TYPE_UNSUPPORTED,
            FederationSdkCasingMode::None,
            FederationSdkCasingMode::None,
        ),
    };
    assert_unsupported(
        &resolver
            .do_get_schema_name_case_insensitively(&FakeConnection::new(), "test", &HashMap::new())
            .await,
    );
}

#[tokio::test]
async fn test_do_get_table_name_case_insensitively_default_implementation() {
    let resolver = PlainJdbcCaseResolver {
        base: CaseResolver::new(
            DB_TYPE_UNSUPPORTED,
            FederationSdkCasingMode::None,
            FederationSdkCasingMode::None,
        ),
    };
    assert_unsupported(
        &resolver
            .do_get_table_name_case_insensitively(
                &FakeConnection::new(),
                TEST_SCHEMA_NAME,
                "test",
                &HashMap::new(),
            )
            .await,
    );
}
