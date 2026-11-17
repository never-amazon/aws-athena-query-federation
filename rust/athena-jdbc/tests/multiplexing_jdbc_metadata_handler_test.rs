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
use indexmap::{IndexMap, IndexSet};
use jdbc_support::{FakeConnection, FakeConnectionFactory, Fakes};

const FAKE_DATABASE: &str = "fakedatabase";
const TEST_CATALOG: &str = "testCatalog";
const TEST_SECRET: &str = "testSecret";
const UNSUPPORTED_CATALOG: &str = "unsupportedCatalog";
const MAX_CATALOGS: usize = 100;
const TOO_MANY_CATALOGS: usize = 101;

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
        &format!("{FAKE_DATABASE}://jdbc:{FAKE_DATABASE}://hostname/${{{TEST_SECRET}}}"),
        TEST_SECRET,
    )
    .unwrap()
}

struct RecordingHandler {
    base: JdbcMetadataHandlerBase,
    calls: Mutex<Vec<String>>,
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

    fn record(&self, call: &str, catalog: &str) {
        self.calls.lock().unwrap().push(format!("{call}:{catalog}"));
    }

    fn calls(&self) -> Vec<String> {
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
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.record("doListSchemaNames", request.catalog_name());
        Ok(ListSchemasResponse::new(request.catalog_name(), Vec::new()))
    }

    async fn do_list_tables(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.record("doListTables", request.catalog_name());
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            Vec::new(),
            None,
        ))
    }

    async fn do_get_query_passthrough_schema(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.record("doGetQueryPassthroughSchema", request.catalog_name());
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            Schema::empty(),
            IndexSet::new(),
        ))
    }

    async fn do_get_table(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.record("doGetTable", request.catalog_name());
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
        self.record("doGetTableLayout", request.catalog_name());
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
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        self.record("getPartitions", request.catalog_name());
        Ok(())
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        self.record("doGetSplits", request.catalog_name());
        Ok(GetSplitsResponse::new(request.catalog_name(), [], None))
    }

    async fn do_get_data_source_capabilities(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.record("doGetDataSourceCapabilities", request.catalog_name());
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
        self.record("getPartitionSchema", catalog_name);
        Ok(Schema::empty())
    }

    async fn close(&self) -> Result<()> {
        self.record("close", "");
        match &self.close_error {
            Some(message) => Err(FederationError::Runtime(message.clone())),
            None => Ok(()),
        }
    }
}

struct Fixture {
    handler: MultiplexingJdbcMetadataHandler,
    delegate: Arc<RecordingHandler>,
    factory: FakeConnectionFactory,
    allocator: Arc<dyn BlockAllocator>,
}

fn mux(
    factory: &FakeConnectionFactory,
    map: IndexMap<String, Arc<dyn JdbcMetadataHandler>>,
) -> Result<MultiplexingJdbcMetadataHandler> {
    let base = JdbcMetadataHandlerBase::new(
        metadata_base(),
        connection_config(),
        Arc::new(factory.clone()),
    );
    MultiplexingJdbcMetadataHandler::new(base, map)
}

fn setup() -> Fixture {
    let delegate = Arc::new(RecordingHandler::new());
    let factory = FakeConnectionFactory::new(FakeConnection::new());
    let mut map: IndexMap<String, Arc<dyn JdbcMetadataHandler>> = IndexMap::new();
    map.insert(FAKE_DATABASE.to_string(), delegate.clone());
    Fixture {
        handler: mux(&factory, map).unwrap(),
        delegate,
        factory,
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
    fixture
        .handler
        .do_list_schema_names(
            &fixture.allocator,
            &ListSchemasRequest::new(None, None, FAKE_DATABASE),
        )
        .await
        .unwrap();
    assert_eq!(fixture.delegate.calls(), ["doListSchemaNames:fakedatabase"]);
}

#[tokio::test]
async fn do_list_tables() {
    let fixture = setup();
    fixture
        .handler
        .do_list_tables(
            &fixture.allocator,
            &ListTablesRequest::new(None, None, FAKE_DATABASE, None, None, -1),
        )
        .await
        .unwrap();
    assert_eq!(fixture.delegate.calls(), ["doListTables:fakedatabase"]);
}

#[tokio::test]
async fn do_get_table() {
    let fixture = setup();
    fixture
        .handler
        .do_get_table(&fixture.allocator, &table_request())
        .await
        .unwrap();
    assert_eq!(fixture.delegate.calls(), ["doGetTable:fakedatabase"]);
}

#[tokio::test]
async fn do_get_table_layout() {
    let fixture = setup();
    fixture
        .handler
        .do_get_table_layout(&fixture.allocator, &layout_request())
        .await
        .unwrap();
    assert_eq!(fixture.delegate.calls(), ["doGetTableLayout:fakedatabase"]);
}

#[tokio::test]
async fn get_partition_schema() {
    let fixture = setup();
    fixture.handler.get_partition_schema(FAKE_DATABASE).unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        ["getPartitionSchema:fakedatabase"]
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
    let mut block = fixture.allocator.create_block(Arc::new(Schema::empty()));
    let mut writer = SimpleBlockWriter::new(&mut block);
    fixture
        .handler
        .get_partitions(&mut writer, &layout_request(), &checker)
        .await
        .unwrap();
    checker.close();
    assert_eq!(fixture.delegate.calls(), ["getPartitions:fakedatabase"]);
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
    assert_eq!(fixture.delegate.calls(), ["doGetSplits:fakedatabase"]);
}

#[tokio::test]
async fn test_constructor_with_too_many_handlers_should_throw_exception() {
    let fixture = setup();
    let mut map: IndexMap<String, Arc<dyn JdbcMetadataHandler>> = IndexMap::new();
    for i in 0..TOO_MANY_CATALOGS {
        map.insert(format!("catalog{i}"), fixture.delegate.clone());
    }
    let Err(error) = mux(&fixture.factory, map) else {
        panic!("expected an error");
    };
    let message = error
        .as_connector_exception()
        .unwrap()
        .message()
        .to_string();
    assert!(message.contains(&format!(
        "Max {MAX_CATALOGS} catalogs supported in multiplexer"
    )));
}

#[tokio::test]
async fn test_do_get_query_passthrough_schema() {
    let fixture = setup();
    fixture
        .handler
        .do_get_query_passthrough_schema(&fixture.allocator, &table_request())
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        ["doGetQueryPassthroughSchema:fakedatabase"]
    );
}

#[tokio::test]
async fn test_do_get_data_source_capabilities() {
    let fixture = setup();
    fixture
        .handler
        .do_get_data_source_capabilities(
            &fixture.allocator,
            &GetDataSourceCapabilitiesRequest::new(None, None, FAKE_DATABASE),
        )
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        ["doGetDataSourceCapabilities:fakedatabase"]
    );
}

#[tokio::test]
async fn test_close_closes_delegates_and_own_factory() {
    let fixture = setup();
    fixture.handler.close().await.unwrap();
    assert_eq!(fixture.delegate.calls(), ["close:"]);
    assert_eq!(fixture.factory.close_calls(), 1);
}

#[tokio::test]
async fn test_close_closes_remaining_delegates_when_one_fails() {
    let failing = Arc::new(RecordingHandler::with_close_error(Some("boom")));
    let healthy = Arc::new(RecordingHandler::new());
    let mut map: IndexMap<String, Arc<dyn JdbcMetadataHandler>> = IndexMap::new();
    map.insert("failing".to_string(), failing.clone());
    map.insert("healthy".to_string(), healthy.clone());
    let own_factory = FakeConnectionFactory::new(FakeConnection::new());
    let handler = mux(&own_factory, map).unwrap();

    let error = handler.close().await.unwrap_err();
    assert!(error.is_runtime_exception());
    assert_eq!(error.to_string(), "boom");
    assert_eq!(failing.calls(), ["close:"]);
    assert_eq!(healthy.calls(), ["close:"]);
    assert_eq!(own_factory.close_calls(), 1);
}
