#[path = "../../athena-jdbc/tests/jdbc_support/mod.rs"]
mod jdbc_support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, BlockWriter, SimpleBlockWriter,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::security::{CachableSecretsManager, KmsEncryptionProvider};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::connection::DatabaseConnectionConfig;
use athena_jdbc::manager::{JdbcMetadataHandler, JdbcMetadataHandlerBase};
use athena_jdbc::MultiplexingJdbcMetadataHandler;
use athena_postgresql::PostGreSqlMuxMetadataHandler;
use indexmap::{IndexMap, IndexSet};
use jdbc_support::{addr, FakeConnection, FakeConnectionFactory, Fakes};

const FAKE_DATABASE: &str = "postgres";
const TEST_CATALOG: &str = "testCatalog";
const TEST_SECRET: &str = "testSecret";
const UNSUPPORTED_CATALOG: &str = "unsupportedCatalog";

fn metadata_base() -> MetadataHandlerBase {
    MetadataHandlerBase::builder(FAKE_DATABASE, Fakes::new().clients())
        .config_options(HashMap::new())
        .build()
        .unwrap()
}

fn connection_config() -> DatabaseConnectionConfig {
    DatabaseConnectionConfig::new_with_secret(
        TEST_CATALOG,
        FAKE_DATABASE,
        "postgres://jdbc:postgresql://hostname/${testSecret}",
        TEST_SECRET,
    )
    .unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    method: &'static str,
    catalog: String,
    args: Vec<usize>,
}

fn call(method: &'static str, catalog: &str, args: &[usize]) -> Call {
    Call {
        method,
        catalog: catalog.to_string(),
        args: args.to_vec(),
    }
}

fn allocator_addr(allocator: &Arc<dyn BlockAllocator>) -> usize {
    addr(&**allocator)
}

struct RecordingHandler {
    base: JdbcMetadataHandlerBase,
    calls: Mutex<Vec<Call>>,
    close_error: Option<String>,
}

impl RecordingHandler {
    fn new() -> RecordingHandler {
        RecordingHandler::with_close_error(None)
    }

    fn with_close_error(close_error: Option<&str>) -> RecordingHandler {
        RecordingHandler {
            base: JdbcMetadataHandlerBase::new_without_connection(metadata_base()),
            calls: Mutex::new(Vec::new()),
            close_error: close_error.map(ToString::to_string),
        }
    }

    fn record(&self, method: &'static str, catalog: &str, args: &[usize]) {
        self.calls.lock().unwrap().push(call(method, catalog, args));
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

impl FederationRequestHandler for RecordingHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.metadata_base().cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.metadata_base().kms_encryption_provider()
    }
}

#[async_trait]
impl MetadataHandler for RecordingHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        self.base.metadata_base()
    }

    async fn do_list_schema_names(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.record(
            "doListSchemaNames",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(ListSchemasResponse::new(request.catalog_name(), Vec::new()))
    }

    async fn do_list_tables(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.record(
            "doListTables",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            Vec::new(),
            None,
        ))
    }

    async fn do_get_query_passthrough_schema(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.record(
            "doGetQueryPassthroughSchema",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            Schema::empty(),
            IndexSet::new(),
        ))
    }

    async fn do_get_table(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.record(
            "doGetTable",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            Schema::empty(),
            IndexSet::new(),
        ))
    }

    async fn do_get_table_layout(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        self.record(
            "doGetTableLayout",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(GetTableLayoutResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            allocator.create_block(Arc::new(Schema::empty())),
        ))
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut (dyn BlockWriter + Send),
        request: &GetTableLayoutRequest,
        query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.record(
            "getPartitions",
            request.catalog_name(),
            &[addr(request), addr(query_status_checker)],
        );
        Ok(())
    }

    async fn do_get_splits(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        self.record(
            "doGetSplits",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(GetSplitsResponse::new(request.catalog_name(), [], None))
    }

    async fn do_get_data_source_capabilities(
        &self,
        allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.record(
            "doGetDataSourceCapabilities",
            request.catalog_name(),
            &[allocator_addr(allocator), addr(request)],
        );
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            IndexMap::new(),
        ))
    }
}

#[async_trait]
impl JdbcMetadataHandler for RecordingHandler {
    fn jdbc_metadata_base(&self) -> &JdbcMetadataHandlerBase {
        &self.base
    }

    fn get_partition_schema(&self, catalog_name: &str) -> Result<Schema> {
        self.record("getPartitionSchema", catalog_name, &[]);
        Ok(Schema::empty())
    }

    async fn close(&self) -> Result<()> {
        self.record("close", "", &[]);
        match &self.close_error {
            Some(message) => Err(FederationError::Runtime(message.clone())),
            None => Ok(()),
        }
    }
}

struct Fixture {
    handler: MultiplexingJdbcMetadataHandler,
    delegate: Arc<RecordingHandler>,
    allocator: Arc<dyn BlockAllocator>,
}

fn mux(
    factory: &FakeConnectionFactory,
    map: IndexMap<String, Arc<dyn JdbcMetadataHandler>>,
) -> Result<MultiplexingJdbcMetadataHandler> {
    PostGreSqlMuxMetadataHandler::with_handler_map(
        Fakes::new().clients(),
        Arc::new(factory.clone()),
        map,
        connection_config(),
        HashMap::new(),
    )
}

fn setup() -> Fixture {
    let delegate = Arc::new(RecordingHandler::new());
    let factory = FakeConnectionFactory::new(FakeConnection::new());
    let mut map: IndexMap<String, Arc<dyn JdbcMetadataHandler>> = IndexMap::new();
    map.insert(FAKE_DATABASE.to_string(), delegate.clone());
    Fixture {
        handler: mux(&factory, map).unwrap(),
        delegate,
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn table_name() -> TableName {
    TableName::new("testSchema", "testTable")
}

fn table_request() -> GetTableRequest {
    GetTableRequest::new(None, None, FAKE_DATABASE, table_name(), HashMap::new())
}

fn layout_request() -> GetTableLayoutRequest {
    GetTableLayoutRequest::new(
        None,
        None,
        FAKE_DATABASE,
        table_name(),
        Constraints::empty(),
        Schema::empty(),
        Vec::<String>::new(),
    )
}

#[tokio::test]
async fn do_list_schema_names() {
    let fixture = setup();
    let request = ListSchemasRequest::new(None, None, FAKE_DATABASE);
    fixture
        .handler
        .do_list_schema_names(&fixture.allocator, &request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doListSchemaNames",
            FAKE_DATABASE,
            &[allocator_addr(&fixture.allocator), addr(&request)]
        )]
    );
}

#[tokio::test]
async fn do_list_tables() {
    let fixture = setup();
    let request = ListTablesRequest::new(None, None, FAKE_DATABASE, None, None, -1);
    fixture
        .handler
        .do_list_tables(&fixture.allocator, &request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doListTables",
            FAKE_DATABASE,
            &[allocator_addr(&fixture.allocator), addr(&request)]
        )]
    );
}

#[tokio::test]
async fn do_get_table() {
    let fixture = setup();
    let request = table_request();
    fixture
        .handler
        .do_get_table(&fixture.allocator, &request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doGetTable",
            FAKE_DATABASE,
            &[allocator_addr(&fixture.allocator), addr(&request)]
        )]
    );
}

#[tokio::test]
async fn do_get_table_layout() {
    let fixture = setup();
    let request = layout_request();
    fixture
        .handler
        .do_get_table_layout(&fixture.allocator, &request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doGetTableLayout",
            FAKE_DATABASE,
            &[allocator_addr(&fixture.allocator), addr(&request)]
        )]
    );
}

#[tokio::test]
async fn get_partition_schema() {
    let fixture = setup();
    fixture.handler.get_partition_schema(FAKE_DATABASE).unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call("getPartitionSchema", FAKE_DATABASE, &[])]
    );
}

#[tokio::test]
async fn get_partition_schema_for_unsupported_catalog() {
    let fixture = setup();
    let error = fixture
        .handler
        .get_partition_schema(UNSUPPORTED_CATALOG)
        .unwrap_err();
    assert!(error.is_runtime_exception());
    assert_eq!(
        error.as_connector_exception().unwrap().message(),
        "Catalog is not supported in multiplexer. After registering the catalog in Athena, must set 'unsupportedCatalog_connection_string' environment variable in Lambda. See JDBC connector README for further details."
    );
    assert!(fixture.delegate.calls().is_empty());
}

#[tokio::test]
async fn get_partitions() {
    let fixture = setup();
    let base = metadata_base();
    let checker = QueryStatusChecker::new(
        Arc::clone(base.athena()),
        Arc::clone(base.athena_invoker()),
        "queryId",
    );
    let request = layout_request();
    let mut block = fixture.allocator.create_block(Arc::new(Schema::empty()));
    let mut writer = SimpleBlockWriter::new(&mut block);
    fixture
        .handler
        .get_partitions(&mut writer, &request, &checker)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "getPartitions",
            FAKE_DATABASE,
            &[addr(&request), addr(&checker)]
        )]
    );
    drop(checker);
}

#[tokio::test]
async fn do_get_splits() {
    let fixture = setup();
    let request = GetSplitsRequest::new(
        None,
        None,
        FAKE_DATABASE,
        table_name(),
        fixture.allocator.create_block(Arc::new(Schema::empty())),
        Vec::new(),
        Constraints::empty(),
        None,
    );
    fixture
        .handler
        .do_get_splits(&fixture.allocator, &request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doGetSplits",
            FAKE_DATABASE,
            &[allocator_addr(&fixture.allocator), addr(&request)]
        )]
    );
}
