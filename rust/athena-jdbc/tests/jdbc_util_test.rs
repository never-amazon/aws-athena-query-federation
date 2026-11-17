mod jdbc_support;

use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::data::{BlockAllocator, BlockSpiller, BlockWriter};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    FederationRequestHandler, MetadataHandler, MetadataHandlerBase, RecordHandler,
    RecordHandlerBase,
};
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest, GetTableRequest, GetTableResponse,
    ListSchemasRequest, ListSchemasResponse, ListTablesRequest, ListTablesResponse,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{
    JdbcMetadataHandler, JdbcMetadataHandlerBase, JdbcMetadataHandlerExt,
    JdbcMetadataHandlerFactory, JdbcRecordHandler, JdbcRecordHandlerBase, JdbcRecordHandlerExt,
    JdbcRecordHandlerFactory, JdbcUtil,
};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement, SqlException};
use jdbc_support::{FakeConnection, Fakes, ResultSetData};

const CONNECTION_STRING1: &str = "fakedatabase://jdbc:fakedatabase://hostname/${testSecret}";
const CONNECTION_STRING2: &str =
    "notrealdb://jdbc:notrealdb://hostname/user=testUser&password=testPassword";
const DEFAULT: &str = "default";
const FAKE_DATABASE: &str = "fakedatabase";
const TEST_SCHEMA: &str = "testSchema";
const TEST_TABLE: &str = "testTable";

struct FakeDatabaseJdbcMetadataHandler {
    base: JdbcMetadataHandlerBase,
}

impl FederationRequestHandler for FakeDatabaseJdbcMetadataHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl MetadataHandler for FakeDatabaseJdbcMetadataHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        self.base.metadata_base()
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.jdbc_do_list_schema_names(allocator, request).await
    }

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.jdbc_do_list_tables(allocator, request).await
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.jdbc_do_get_table(allocator, request).await
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut (dyn BlockWriter + Send),
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Ok(())
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        Ok(GetSplitsResponse::new(
            request.catalog_name(),
            Vec::<Split>::new(),
            None,
        ))
    }
}

impl JdbcMetadataHandler for FakeDatabaseJdbcMetadataHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    fn get_partition_schema(&self, _catalog_name: &str) -> Result<Schema> {
        Ok(Schema::empty())
    }
}

struct FakeDatabaseJdbcRecordHandler {
    base: JdbcRecordHandlerBase,
}

impl FederationRequestHandler for FakeDatabaseJdbcRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.record_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.record_base().kms_encryption_provider()
    }

    fn database_connection_secret(&self) -> Option<&str> {
        self.jdbc_database_connection_secret()
    }
}

#[async_trait]
impl RecordHandler for FakeDatabaseJdbcRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        self.base.record_base()
    }

    async fn read_with_constraint(
        &self,
        block_spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.jdbc_read_with_constraint(block_spiller, request, query_status_checker)
            .await
    }
}

#[async_trait]
impl JdbcRecordHandler for FakeDatabaseJdbcRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn build_split_sql(
        &self,
        _jdbc_connection: &dyn JdbcConnection,
        _catalog_name: &str,
        _table_name: &TableName,
        _schema: &Schema,
        _constraints: &Constraints,
        _split: &Split,
    ) -> Result<PreparedStatement> {
        Ok(PreparedStatement::new(""))
    }
}

struct FakeDatabaseMetadataHandlerFactory;

#[async_trait]
impl JdbcMetadataHandlerFactory for FakeDatabaseMetadataHandlerFactory {
    fn engine(&self) -> &str {
        FAKE_DATABASE
    }

    async fn create_jdbc_metadata_handler(
        &self,
        _config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcMetadataHandler>> {
        let metadata_base = MetadataHandlerBase::builder(FAKE_DATABASE, Fakes::new().clients())
            .config_options(config_options.clone())
            .build()?;
        Ok(Arc::new(FakeDatabaseJdbcMetadataHandler {
            base: JdbcMetadataHandlerBase::new_without_connection(metadata_base),
        }))
    }
}

struct FakeDatabaseRecordHandlerFactory;

#[async_trait]
impl JdbcRecordHandlerFactory for FakeDatabaseRecordHandlerFactory {
    fn engine(&self) -> &str {
        FAKE_DATABASE
    }

    async fn create_jdbc_record_handler(
        &self,
        _config: &DatabaseConnectionConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<Arc<dyn JdbcRecordHandler>> {
        let record_base = RecordHandlerBase::new(
            FAKE_DATABASE,
            Fakes::new().clients(),
            config_options.clone(),
        )?;
        Ok(Arc::new(FakeDatabaseJdbcRecordHandler {
            base: JdbcRecordHandlerBase::new_without_connection(record_base),
        }))
    }
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
    let catalogs = JdbcUtil::create_jdbc_metadata_handler_map(
        &options(&[
            ("testCatalog1_connection_string", CONNECTION_STRING1),
            (DEFAULT, CONNECTION_STRING1),
            ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
        ]),
        &FakeDatabaseMetadataHandlerFactory,
    )
    .await
    .unwrap();
    assert_eq!(3, catalogs.len());
    assert_eq!(
        FAKE_DATABASE,
        catalogs["testCatalog1"].metadata_base().source_type()
    );
    assert_eq!(
        FAKE_DATABASE,
        catalogs["lambda:functionName"]
            .metadata_base()
            .source_type()
    );
    assert!(Arc::ptr_eq(
        &catalogs[DEFAULT],
        &catalogs["lambda:functionName"]
    ));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_map_different_databases_throws() {
    let result = JdbcUtil::create_jdbc_metadata_handler_map(
        &options(&[
            ("testCatalog_connection_string", CONNECTION_STRING1),
            ("wrong_connection_string", CONNECTION_STRING2),
            (DEFAULT, CONNECTION_STRING1),
            ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
        ]),
        &FakeDatabaseMetadataHandlerFactory,
    )
    .await;
    assert!(matches!(result, Err(FederationError::IllegalArgument(_))));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_empty_connection_strings() {
    let result = JdbcUtil::create_jdbc_metadata_handler_map(
        &HashMap::new(),
        &FakeDatabaseMetadataHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_metadata_handler_no_default() {
    let result = JdbcUtil::create_jdbc_metadata_handler_map(
        &options(&[
            ("testCatalog1_connection_string", CONNECTION_STRING1),
            ("testCatalog2_connection_string", CONNECTION_STRING2),
            (DEFAULT, CONNECTION_STRING2),
            ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
        ]),
        &FakeDatabaseMetadataHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_record_handler_map() {
    let catalogs = JdbcUtil::create_jdbc_record_handler_map(
        &options(&[
            ("testCatalog1_connection_string", CONNECTION_STRING1),
            (DEFAULT, CONNECTION_STRING1),
            ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
        ]),
        &FakeDatabaseRecordHandlerFactory,
    )
    .await
    .unwrap();
    assert_eq!(
        FAKE_DATABASE,
        catalogs["testCatalog1"].record_base().source_type()
    );
    assert_eq!(
        FAKE_DATABASE,
        catalogs["lambda:functionName"].record_base().source_type()
    );
}

#[tokio::test]
async fn create_jdbc_record_handler_map_different_databases_throws() {
    let result = JdbcUtil::create_jdbc_record_handler_map(
        &options(&[
            ("testCatalog_connection_string", CONNECTION_STRING1),
            ("wrong_connection_string", CONNECTION_STRING2),
            (DEFAULT, CONNECTION_STRING1),
            ("AWS_LAMBDA_FUNCTION_NAME", "functionName"),
        ]),
        &FakeDatabaseRecordHandlerFactory,
    )
    .await;
    assert!(matches!(result, Err(FederationError::IllegalArgument(_))));
}

#[tokio::test]
async fn create_jdbc_record_handler_map_empty_connection_strings() {
    let result = JdbcUtil::create_jdbc_record_handler_map(
        &HashMap::new(),
        &FakeDatabaseRecordHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn create_jdbc_record_handler_map_no_default() {
    let result = JdbcUtil::create_jdbc_record_handler_map(
        &options(&[
            ("testCatalog1_connection_string", CONNECTION_STRING1),
            ("testCatalog2_connection_string", CONNECTION_STRING2),
        ]),
        &FakeDatabaseRecordHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[test]
fn test_get_single_database_config_from_env() {
    let config = JdbcUtil::get_single_database_config_from_env(
        FAKE_DATABASE,
        &options(&[(DEFAULT, CONNECTION_STRING1)]),
    )
    .unwrap();
    assert_eq!(DEFAULT, config.catalog());
    assert_eq!(FAKE_DATABASE, config.engine());
}

#[test]
fn test_get_single_database_config_from_env_throws_when_no_default() {
    let result = JdbcUtil::get_single_database_config_from_env(
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
    let tables = JdbcUtil::get_tables(&connection, TEST_SCHEMA)
        .await
        .unwrap();
    assert_eq!(1, tables.len());
    assert_eq!(TEST_TABLE, tables[0].table_name());
    assert_eq!(TEST_SCHEMA, tables[0].schema_name());
    assert_eq!(
        Some(TEST_SCHEMA),
        connection.executed()[0]
            .parameter(1)
            .and_then(athena_jdbc::sql::SqlValue::to_java_string)
            .as_deref()
    );
}

#[tokio::test]
async fn test_get_table_metadata_with_sql_exception() {
    let connection = FakeConnection::new();
    connection.on_query(|_| Err(SqlException::new("Test exception")));
    let thrown = JdbcUtil::get_table_metadata(&connection, &statement(), "Tables")
        .await
        .unwrap_err();
    assert_eq!("Test exception", thrown.message());
}

#[tokio::test]
async fn test_get_table_metadata_with_no_warehouse_selected_propagates() {
    let connection = FakeConnection::new();
    connection.on_query(|_| {
        Err(SqlException::new(
            "No active warehouse selected in the current session.  Select an active warehouse with the 'use warehouse' command.",
        )
        .with_sql_state("57P03")
        .with_error_code(606))
    });
    let thrown = JdbcUtil::get_table_metadata(&connection, &statement(), "Tables and Views")
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
        JdbcUtil::get_table_metadata(&connection, &statement(), "Tables")
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
    let table_name = JdbcUtil::get_schema_table_name(&result_set).unwrap();
    assert_eq!(TEST_SCHEMA, table_name.schema_name());
    assert_eq!(TEST_TABLE, table_name.table_name());
}

#[tokio::test]
async fn test_create_jdbc_metadata_handler_map_missing_lambda_function_name() {
    let result = JdbcUtil::create_jdbc_metadata_handler_map(
        &options(&[(DEFAULT, CONNECTION_STRING1)]),
        &FakeDatabaseMetadataHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}

#[tokio::test]
async fn test_create_jdbc_record_handler_map_missing_lambda_function_name() {
    let result = JdbcUtil::create_jdbc_record_handler_map(
        &options(&[(DEFAULT, CONNECTION_STRING1)]),
        &FakeDatabaseRecordHandlerFactory,
    )
    .await;
    assert!(result.err().is_some_and(|e| e.is_runtime_exception()));
}
