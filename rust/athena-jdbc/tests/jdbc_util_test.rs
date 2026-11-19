mod jdbc_support;

use athena_federation_sdk::resolver::FederationSdkCasingMode;
use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::data::SimpleBlockWriter;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::handlers::{
    HandlerContext, MetadataContext, MetadataHandler, RecordHandler,
};
use athena_federation_sdk::metadata::{GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{
    jdbc_util, ConnectionSource, JdbcMetadataHandler, JdbcRecordHandler, MetadataDialect,
    RecordDialect, SplitQuery,
};
use athena_jdbc::resolver::JdbcCaseResolver;
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, SqlError};
use jdbc_support::{FakeConnection, FakeConnectionFactory, Fakes, ResultSetData};

const CONNECTION_STRING1: &str = "fakedatabase://jdbc:fakedatabase://hostname/${testSecret}";
const CONNECTION_STRING2: &str =
    "notrealdb://jdbc:notrealdb://hostname/user=testUser&password=testPassword";
const DEFAULT: &str = "default";
const FAKE_DATABASE: &str = "fakedatabase";
const TEST_SCHEMA: &str = "testSchema";
const TEST_TABLE: &str = "testTable";

struct FakeDatabaseMetadataDialect;

#[async_trait]
impl MetadataDialect for FakeDatabaseMetadataDialect {
    fn partition_schema(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        _catalog_name: &str,
    ) -> Result<Schema> {
        Ok(Schema::empty())
    }

    async fn get_partitions(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        _block_writer: &mut SimpleBlockWriter<'_>,
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_splits(
        &self,
        _handler: &JdbcMetadataHandler<Self>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        Ok(GetSplitsResponse::new(
            request.catalog_name(),
            Vec::<Split>::new(),
            None,
        ))
    }
}

type FakeDatabaseJdbcMetadataHandler = JdbcMetadataHandler<FakeDatabaseMetadataDialect>;

struct FakeDatabaseRecordDialect;

#[async_trait]
impl RecordDialect for FakeDatabaseRecordDialect {
    async fn build_split_sql(
        &self,
        _handler: &JdbcRecordHandler<Self>,
        _jdbc_connection: &dyn JdbcConnection,
        _query: &SplitQuery<'_>,
    ) -> Result<PreparedStatement> {
        Ok(PreparedStatement::new(""))
    }
}

type FakeDatabaseJdbcRecordHandler = JdbcRecordHandler<FakeDatabaseRecordDialect>;

fn metadata_handler_map(
    config_options: &HashMap<String, String>,
) -> Result<IndexMap<String, Arc<FakeDatabaseJdbcMetadataHandler>>> {
    jdbc_util::handler_map(
        FAKE_DATABASE,
        config_options,
        |config: &DatabaseConnectionConfig| {
            let context = MetadataContext::builder(FAKE_DATABASE, Fakes::new().clients())
                .config_options(config_options.clone())
                .build()?;
            Ok(Arc::new(JdbcMetadataHandler::new(
                context,
                ConnectionSource::Direct {
                    config: config.clone(),
                    factory: Arc::new(FakeConnectionFactory::default()),
                },
                JdbcCaseResolver::new(
                    FAKE_DATABASE,
                    FederationSdkCasingMode::None,
                    FederationSdkCasingMode::None,
                ),
                FakeDatabaseMetadataDialect,
            )))
        },
    )
}

fn record_handler_map(
    config_options: &HashMap<String, String>,
) -> Result<IndexMap<String, Arc<FakeDatabaseJdbcRecordHandler>>> {
    jdbc_util::handler_map(
        FAKE_DATABASE,
        config_options,
        |_config: &DatabaseConnectionConfig| {
            let context = HandlerContext::new(
                FAKE_DATABASE,
                Fakes::new().clients(),
                config_options.clone(),
            )?;
            Ok(Arc::new(JdbcRecordHandler::new(
                context,
                ConnectionSource::Delegating,
                FakeDatabaseRecordDialect,
            )))
        },
    )
}

fn options(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn statement() -> PreparedStatement {
    PreparedStatement::new("SELECT 1")
}

#[tokio::test]
async fn create_jdbc_metadata_handler_map() {
    let catalogs = metadata_handler_map(&options(&[
        ("testCatalog1_connection_string", CONNECTION_STRING1),
        (DEFAULT, CONNECTION_STRING1),
        ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
    ]))
    .unwrap();
    assert_eq!(3, catalogs.len());
    assert_eq!(
        FAKE_DATABASE,
        catalogs["testCatalog1"].context().handler().source_type()
    );
    assert_eq!(
        FAKE_DATABASE,
        catalogs["lambda:functionName"]
            .context()
            .handler()
            .source_type()
    );
    assert!(Arc::ptr_eq(
        &catalogs[DEFAULT],
        &catalogs["lambda:functionName"]
    ));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_map_different_databases_throws() {
    let result = metadata_handler_map(&options(&[
        ("testCatalog_connection_string", CONNECTION_STRING1),
        ("wrong_connection_string", CONNECTION_STRING2),
        (DEFAULT, CONNECTION_STRING1),
        ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
    ]));
    assert!(matches!(result, Err(FederationError::IllegalArgument(_))));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_empty_connection_strings() {
    let result = metadata_handler_map(&HashMap::new());
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_no_default() {
    let result = metadata_handler_map(&options(&[
        ("testCatalog1_connection_string", CONNECTION_STRING1),
        ("testCatalog2_connection_string", CONNECTION_STRING2),
        (DEFAULT, CONNECTION_STRING2),
        ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
    ]));
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_record_handler_map() {
    let catalogs = record_handler_map(&options(&[
        ("testCatalog1_connection_string", CONNECTION_STRING1),
        (DEFAULT, CONNECTION_STRING1),
        ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
    ]))
    .unwrap();
    assert_eq!(
        FAKE_DATABASE,
        catalogs["testCatalog1"].context().source_type()
    );
    assert_eq!(
        FAKE_DATABASE,
        catalogs["lambda:functionName"].context().source_type()
    );
}

#[tokio::test]
async fn create_jdbc_record_handler_map_different_databases_throws() {
    let result = record_handler_map(&options(&[
        ("testCatalog_connection_string", CONNECTION_STRING1),
        ("wrong_connection_string", CONNECTION_STRING2),
        (DEFAULT, CONNECTION_STRING1),
        ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
    ]));
    assert!(matches!(result, Err(FederationError::IllegalArgument(_))));
}

#[tokio::test]
async fn create_jdbc_record_handler_map_empty_connection_strings() {
    let result = record_handler_map(&HashMap::new());
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_record_handler_map_no_default() {
    let result = record_handler_map(&options(&[
        ("testCatalog1_connection_string", CONNECTION_STRING1),
        ("testCatalog2_connection_string", CONNECTION_STRING2),
    ]));
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[test]
fn test_get_single_database_config_from_env() {
    let config = jdbc_util::single_database_config_from_env(
        FAKE_DATABASE,
        &options(&[(DEFAULT, CONNECTION_STRING1)]),
    )
    .unwrap();
    assert_eq!(DEFAULT, config.catalog());
    assert_eq!(FAKE_DATABASE, config.engine());
}

#[test]
fn test_get_single_database_config_from_env_throws_when_no_default() {
    let result = jdbc_util::single_database_config_from_env(
        FAKE_DATABASE,
        &options(&[("testCatalog_connection_string", CONNECTION_STRING1)]),
    );
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn test_get_tables() {
    let connection = FakeConnection::new();
    connection.on_query(|_| {
        Ok(ResultSetData::of_strings(
            &["TABLE_NAME", "TABLE_SCHEM"],
            &[&[TEST_TABLE, TEST_SCHEMA]],
        ))
    });
    let tables = jdbc_util::tables(&connection, TEST_SCHEMA).await.unwrap();
    assert_eq!(1, tables.len());
    assert_eq!(TEST_TABLE, tables[0].table_name());
    assert_eq!(TEST_SCHEMA, tables[0].schema_name());
    assert_eq!(
        Some(TEST_SCHEMA),
        connection.executed()[0]
            .parameters()
            .first()
            .and_then(athena_jdbc::sql::SqlValue::to_java_string)
            .as_deref()
    );
}

#[tokio::test]
async fn test_get_table_metadata_with_sql_exception() {
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlError::new("Test exception")));
    let thrown = jdbc_util::table_metadata(&connection, &statement(), "Tables")
        .await
        .unwrap_err();
    assert_eq!("Test exception", thrown.message());
}

#[tokio::test]
async fn test_get_table_metadata_with_no_warehouse_selected_propagates() {
    let connection = FakeConnection::new();
    connection.on_query(|_| {
        Err(SqlError::Jdbc {
            message: "No active warehouse selected in the current session.  Select an active warehouse with the 'use warehouse' command.".to_string(),
            sql_state: Some("57P03".to_string()),
            code: Some(606),
            cause: None,
        })
    });
    let thrown = jdbc_util::table_metadata(&connection, &statement(), "Tables and Views")
        .await
        .unwrap_err();
    assert_eq!(Some("57P03"), thrown.sql_state());
    assert_eq!(606, thrown.error_code());
}

#[tokio::test]
async fn test_get_table_metadata_returns_empty_list_for_empty_schema() {
    let connection = FakeConnection::new();
    connection.on_query(|_| Ok(ResultSetData::empty(&["TABLE_NAME", "TABLE_SCHEM"])));
    assert!(
        jdbc_util::table_metadata(&connection, &statement(), "Tables")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn test_get_schema_table_name() {
    let mut result_set = ResultSetData::of_strings(
        &["TABLE_SCHEM", "TABLE_NAME"],
        &[&[TEST_SCHEMA, TEST_TABLE]],
    )
    .to_result_set();
    assert!(result_set.next().await.unwrap());
    let table_name = jdbc_util::schema_table_name(&result_set).unwrap();
    assert_eq!(TEST_SCHEMA, table_name.schema_name());
    assert_eq!(TEST_TABLE, table_name.table_name());
}

#[tokio::test]
async fn test_create_jdbc_metadata_handler_map_missing_lambda_function_name() {
    let result = metadata_handler_map(&options(&[(DEFAULT, CONNECTION_STRING1)]));
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn test_create_jdbc_record_handler_map_missing_lambda_function_name() {
    let result = record_handler_map(&options(&[(DEFAULT, CONNECTION_STRING1)]));
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}
