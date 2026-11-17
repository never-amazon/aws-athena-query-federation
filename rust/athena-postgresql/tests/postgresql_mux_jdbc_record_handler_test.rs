#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, BlockSpiller, S3BlockSpiller,
};
use athena_federation_sdk::domain::predicate::{ConstraintEvaluator, Constraints};
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{FederationRequestHandler, RecordHandler, RecordHandlerBase};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{JdbcRecordHandler, JdbcRecordHandlerBase};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement};
use athena_jdbc::MultiplexingJdbcRecordHandler;
use athena_postgresql::PostGreSqlMuxRecordHandler;
use indexmap::IndexMap;
use jdbc_support::{FakeConnection, FakeConnectionFactory, Fakes};

const FAKE_DATABASE: &str = "postgres";
const TEST_CATALOG: &str = "testCatalog";
const TEST_SECRET: &str = "testSecret";
const UNSUPPORTED_CATALOG: &str = "unsupportedCatalog";
const CONNECTION_STRING: &str = "postgres://jdbc:postgresql://hostname/${testSecret}";

#[derive(Default)]
struct Calls {
    read_with_constraint: AtomicUsize,
    build_split_sql: Mutex<Vec<(String, TableName)>>,
    close: AtomicUsize,
}

struct FakeJdbcRecordHandler {
    base: JdbcRecordHandlerBase,
    calls: Arc<Calls>,
    close_error: Option<String>,
}

impl FederationRequestHandler for FakeJdbcRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.record_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.record_base().kms_encryption_provider()
    }
}

#[async_trait]
impl RecordHandler for FakeJdbcRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        self.base.record_base()
    }

    async fn read_with_constraint(
        &self,
        _spiller: &dyn BlockSpiller,
        _request: &ReadRecordsRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.calls
            .read_with_constraint
            .fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait]
impl JdbcRecordHandler for FakeJdbcRecordHandler {
    fn jdbc_record_base(&self) -> &JdbcRecordHandlerBase {
        &self.base
    }

    async fn build_split_sql(
        &self,
        _jdbc_connection: &dyn JdbcConnection,
        catalog_name: &str,
        table_name: &TableName,
        _schema: &Schema,
        _constraints: &Constraints,
        _split: &Split,
    ) -> Result<PreparedStatement> {
        self.calls
            .build_split_sql
            .lock()
            .unwrap()
            .push((catalog_name.to_string(), table_name.clone()));
        Ok(PreparedStatement::new("delegate"))
    }

    async fn close(&self) -> Result<()> {
        self.calls.close.fetch_add(1, Ordering::SeqCst);
        match &self.close_error {
            Some(message) => Err(FederationError::Runtime(message.clone())),
            None => Ok(()),
        }
    }
}

fn record_base(fakes: &Fakes) -> RecordHandlerBase {
    RecordHandlerBase::new(FAKE_DATABASE, fakes.clients(), HashMap::new()).unwrap()
}

fn config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new_with_secret(
        TEST_CATALOG,
        FAKE_DATABASE,
        CONNECTION_STRING,
        TEST_SECRET,
    )
    .unwrap()
}

fn fake_delegate(
    fakes: &Fakes,
    close_error: Option<&str>,
) -> (Arc<dyn JdbcRecordHandler>, Arc<Calls>) {
    let calls = Arc::new(Calls::default());
    let handler = FakeJdbcRecordHandler {
        base: JdbcRecordHandlerBase::new_without_connection(record_base(fakes)),
        calls: Arc::clone(&calls),
        close_error: close_error.map(ToString::to_string),
    };
    (Arc::new(handler), calls)
}

struct Setup {
    fakes: Fakes,
    calls: Arc<Calls>,
    handler: MultiplexingJdbcRecordHandler,
}

fn set_up() -> Setup {
    let fakes = Fakes::new();
    let factory = FakeConnectionFactory::default();
    let (delegate, calls) = fake_delegate(&fakes, None);
    let map = IndexMap::from([(FAKE_DATABASE.to_string(), delegate)]);
    let handler = PostGreSqlMuxRecordHandler::with_handler_map(
        fakes.clients(),
        Arc::new(factory.clone()),
        config(),
        map,
        HashMap::new(),
    )
    .unwrap();
    Setup {
        fakes,
        calls,
        handler,
    }
}

fn request(catalog: &str) -> ReadRecordsRequest {
    ReadRecordsRequest::new(
        None,
        catalog,
        "queryId",
        TableName::new("testSchema", "tableName"),
        Schema::empty(),
        Split::new_builder(None, None).build(),
        Constraints::empty(),
        1024,
        1024,
    )
}

fn spiller(fakes: &Fakes) -> S3BlockSpiller {
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let spill_config = SpillConfig::new_builder()
        .with_spill_location(
            S3SpillLocation::new_builder()
                .with_bucket("bucket")
                .with_query_id("queryId")
                .with_split_id("split")
                .with_is_directory(true)
                .build()
                .into(),
        )
        .with_request_id("queryId")
        .build()
        .unwrap();
    S3BlockSpiller::new(
        Arc::new(fakes.s3.clone()),
        spill_config,
        allocator,
        Arc::new(Schema::empty()),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap()
}

fn checker(s: &Setup) -> QueryStatusChecker {
    QueryStatusChecker::new(
        s.fakes.athena.clone(),
        Arc::clone(s.handler.record_base().athena_invoker()),
        "queryId",
    )
}

#[tokio::test]
async fn read_with_constraint() {
    let s = set_up();
    let block_spiller = spiller(&s.fakes);
    let checker = checker(&s);
    s.handler
        .read_with_constraint(&block_spiller, &request(FAKE_DATABASE), &checker)
        .await
        .unwrap();
    checker.close();
    assert_eq!(1, s.calls.read_with_constraint.load(Ordering::SeqCst));
}

#[tokio::test]
async fn read_with_constraint_with_unsupported_catalog() {
    let s = set_up();
    let block_spiller = spiller(&s.fakes);
    let checker = checker(&s);
    let e = s
        .handler
        .read_with_constraint(&block_spiller, &request(UNSUPPORTED_CATALOG), &checker)
        .await
        .unwrap_err();
    checker.close();
    assert!(e.is_runtime_exception());
    assert_eq!(
        "Catalog is not supported in multiplexer. After registering the catalog in Athena, must set 'unsupportedCatalog_connection_string' environment variable in Lambda. See JDBC connector README for further details.",
        e.as_connector_exception().unwrap().message()
    );
    assert_eq!(0, s.calls.read_with_constraint.load(Ordering::SeqCst));
}

#[tokio::test]
async fn build_split_sql() {
    let s = set_up();
    let table_name = TableName::new("testSchema", "tableName");
    let statement = s
        .handler
        .build_split_sql(
            &FakeConnection::new(),
            FAKE_DATABASE,
            &table_name,
            &Schema::empty(),
            &Constraints::empty(),
            &Split::new_builder(None, None).build(),
        )
        .await
        .unwrap();
    assert_eq!("delegate", statement.sql());
    assert_eq!(
        vec![(FAKE_DATABASE.to_string(), table_name)],
        s.calls.build_split_sql.lock().unwrap().clone()
    );
}
