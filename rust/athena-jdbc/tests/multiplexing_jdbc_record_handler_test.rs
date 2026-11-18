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
use athena_jdbc::connection::{DatabaseConnectionConfig, JdbcConnectionFactory};
use athena_jdbc::manager::{JdbcRecordHandler, JdbcRecordHandlerBase};
use athena_jdbc::sql::{JdbcConnection, PreparedStatement};
use athena_jdbc::MultiplexingJdbcRecordHandler;
use indexmap::IndexMap;
use jdbc_support::{addr, FakeConnection, FakeConnectionFactory, Fakes};

const FAKE_DATABASE: &str = "fakedatabase";
const TEST_CATALOG: &str = "testCatalog";
const TEST_SECRET: &str = "testSecret";
const UNSUPPORTED_CATALOG: &str = "unsupportedCatalog";
const CONNECTION_STRING: &str = "fakedatabase://jdbc:fakedatabase://hostname/${testSecret}";
const MAX_CATALOGS: usize = 100;
const TOO_MANY_CATALOGS: usize = 101;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildSplitSqlCall {
    connection: usize,
    catalog_name: String,
    table_name: TableName,
    schema: usize,
    constraints: usize,
    split: usize,
}

#[derive(Default)]
struct Calls {
    read_with_constraint: Mutex<Vec<[usize; 3]>>,
    build_split_sql: Mutex<Vec<BuildSplitSqlCall>>,
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
        spiller: &dyn BlockSpiller,
        request: &ReadRecordsRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.calls.read_with_constraint.lock().unwrap().push([
            addr(spiller),
            addr(request),
            addr(query_status_checker),
        ]);
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
        jdbc_connection: &dyn JdbcConnection,
        catalog_name: &str,
        table_name: &TableName,
        schema: &Schema,
        constraints: &Constraints,
        split: &Split,
    ) -> Result<PreparedStatement> {
        self.calls
            .build_split_sql
            .lock()
            .unwrap()
            .push(BuildSplitSqlCall {
                connection: addr(jdbc_connection),
                catalog_name: catalog_name.to_string(),
                table_name: table_name.clone(),
                schema: addr(schema),
                constraints: addr(constraints),
                split: addr(split),
            });
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

fn mux_base(fakes: &Fakes, factory: &FakeConnectionFactory) -> JdbcRecordHandlerBase {
    let factory: Arc<dyn JdbcConnectionFactory> = Arc::new(factory.clone());
    JdbcRecordHandlerBase::new(record_base(fakes), config(), factory)
}

struct Setup {
    fakes: Fakes,
    factory: FakeConnectionFactory,
    calls: Arc<Calls>,
    handler: MultiplexingJdbcRecordHandler,
}

fn set_up() -> Setup {
    let fakes = Fakes::new();
    let factory = FakeConnectionFactory::default();
    let (delegate, calls) = fake_delegate(&fakes, None);
    let map = IndexMap::from([(FAKE_DATABASE.to_string(), delegate)]);
    let handler = MultiplexingJdbcRecordHandler::new(mux_base(&fakes, &factory), map).unwrap();
    Setup {
        fakes,
        factory,
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
    let read_records_request = request(FAKE_DATABASE);
    s.handler
        .read_with_constraint(&block_spiller, &read_records_request, &checker)
        .await
        .unwrap();
    assert_eq!(
        vec![[
            addr(&block_spiller),
            addr(&read_records_request),
            addr(&checker)
        ]],
        s.calls.read_with_constraint.lock().unwrap().clone()
    );
    drop(checker);
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
    drop(checker);
    assert!(e.is_runtime_exception());
    assert_eq!(
        "Catalog is not supported in multiplexer. After registering the catalog in Athena, must set 'unsupportedCatalog_connection_string' environment variable in Lambda. See JDBC connector README for further details.",
        e.as_connector_exception().unwrap().message()
    );
    assert!(s.calls.read_with_constraint.lock().unwrap().is_empty());
}

#[tokio::test]
async fn build_split_sql() {
    let s = set_up();
    let jdbc_connection = FakeConnection::new();
    let table_name = TableName::new("testSchema", "tableName");
    let schema = Schema::empty();
    let constraints = Constraints::empty();
    let split = Split::new_builder(None, None).build();
    let statement = s
        .handler
        .build_split_sql(
            &jdbc_connection,
            FAKE_DATABASE,
            &table_name,
            &schema,
            &constraints,
            &split,
        )
        .await
        .unwrap();
    assert_eq!("delegate", statement.sql());
    assert_eq!(
        vec![BuildSplitSqlCall {
            connection: addr(&jdbc_connection),
            catalog_name: "fakedatabase".to_string(),
            table_name,
            schema: addr(&schema),
            constraints: addr(&constraints),
            split: addr(&split),
        }],
        s.calls.build_split_sql.lock().unwrap().clone()
    );
}

#[test]
fn test_constructor_with_too_many_handlers_should_throw_exception() {
    let fakes = Fakes::new();
    let (delegate, _) = fake_delegate(&fakes, None);
    let map: IndexMap<String, Arc<dyn JdbcRecordHandler>> = (0..TOO_MANY_CATALOGS)
        .map(|i| (format!("catalog{i}"), Arc::clone(&delegate)))
        .collect();
    let Err(e) = MultiplexingJdbcRecordHandler::new(
        mux_base(&fakes, &FakeConnectionFactory::default()),
        map,
    ) else {
        panic!("expected an error");
    };
    assert!(e.as_connector_exception().is_some());
    assert!(e.to_string().contains(&format!(
        "Max {MAX_CATALOGS} catalogs supported in multiplexer"
    )));
}

#[test]
fn test_constructor_with_empty_handler_map_should_throw_exception() {
    let fakes = Fakes::new();
    let Err(e) = MultiplexingJdbcRecordHandler::new(
        mux_base(&fakes, &FakeConnectionFactory::default()),
        IndexMap::new(),
    ) else {
        panic!("expected an error");
    };
    assert!(matches!(&e, FederationError::IllegalArgument(_)));
    assert!(e.to_string().contains("recordHandlerMap must not be empty"));
}

#[test]
fn test_constructor_with_null_handler_map_should_throw_exception() {
    let fakes = Fakes::new();
    let Err(e) = MultiplexingJdbcRecordHandler::new(
        mux_base(&fakes, &FakeConnectionFactory::default()),
        IndexMap::<String, Arc<dyn JdbcRecordHandler>>::new(),
    ) else {
        panic!("expected an error");
    };
    assert!(e.to_string().contains("recordHandlerMap must not be empty"));
}

#[tokio::test]
async fn test_close_closes_delegates_and_own_factory() {
    let s = set_up();
    s.handler.close().await.unwrap();
    assert_eq!(1, s.calls.close.load(Ordering::SeqCst));
    assert_eq!(1, s.factory.close_calls());
}

#[tokio::test]
async fn test_close_closes_remaining_delegates_when_one_fails() {
    let fakes = Fakes::new();
    let (failing, failing_calls) = fake_delegate(&fakes, Some("boom"));
    let (healthy, healthy_calls) = fake_delegate(&fakes, None);
    let map = IndexMap::from([
        ("failing".to_string(), failing),
        ("healthy".to_string(), healthy),
    ]);
    let own_factory = FakeConnectionFactory::default();
    let handler = MultiplexingJdbcRecordHandler::new(mux_base(&fakes, &own_factory), map).unwrap();

    let e = handler.close().await.unwrap_err();
    assert!(e.is_runtime_exception());
    assert_eq!("boom", e.to_string());
    assert_eq!(1, failing_calls.close.load(Ordering::SeqCst));
    assert_eq!(1, healthy_calls.close.load(Ordering::SeqCst));
    assert_eq!(1, own_factory.close_calls());
}
