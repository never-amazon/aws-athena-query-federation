mod jdbc_support;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::datatypes::Schema;
use async_trait::async_trait;
use athena_federation_sdk::data::{BlockAllocator, BlockAllocatorImpl, SimpleBlockWriter};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::handlers::{MetadataContext, MetadataHandler};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetDataSourceCapabilitiesResponse, GetSplitsRequest,
    GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse, GetTableRequest,
    GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use athena_jdbc::manager::MetadataDelegate;
use athena_jdbc::MultiplexingJdbcMetadataHandler;
use indexmap::{IndexMap, IndexSet};
use jdbc_support::{addr, FakeConnection, FakeConnectionFactory, Fakes};

const FAKE_DATABASE: &str = "fakedatabase";
const UNSUPPORTED_CATALOG: &str = "unsupportedCatalog";
const MAX_CATALOGS: usize = 100;
const TOO_MANY_CATALOGS: usize = 101;

fn metadata_context() -> MetadataContext {
    MetadataContext::builder(FAKE_DATABASE, Fakes::new().clients())
        .config_options(HashMap::new())
        .build()
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
    context: MetadataContext,
    calls: Mutex<Vec<Call>>,
    close_error: Option<String>,
}

impl RecordingHandler {
    fn new() -> RecordingHandler {
        RecordingHandler::with_close_error(None)
    }

    fn with_close_error(close_error: Option<&str>) -> RecordingHandler {
        RecordingHandler {
            context: metadata_context(),
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

#[async_trait]
impl MetadataHandler for RecordingHandler {
    fn context(&self) -> &MetadataContext {
        &self.context
    }

    async fn list_schema_names(&self, request: &ListSchemasRequest) -> Result<ListSchemasResponse> {
        self.record(
            "doListSchemaNames",
            request.catalog_name(),
            &[addr(request)],
        );
        Ok(ListSchemasResponse::new(request.catalog_name(), Vec::new()))
    }

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse> {
        self.record("doListTables", request.catalog_name(), &[addr(request)]);
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            Vec::new(),
            None,
        ))
    }

    async fn get_query_passthrough_schema(
        &self,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.record(
            "doGetQueryPassthroughSchema",
            request.catalog_name(),
            &[addr(request)],
        );
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            Schema::empty(),
            IndexSet::new(),
        ))
    }

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse> {
        self.record("doGetTable", request.catalog_name(), &[addr(request)]);
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            Schema::empty(),
            IndexSet::new(),
        ))
    }

    async fn get_table_layout(
        &self,
        allocator: &dyn BlockAllocator,
        request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        self.record(
            "doGetTableLayout",
            request.catalog_name(),
            &[addr(allocator), addr(request)],
        );
        Ok(GetTableLayoutResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            allocator.create_block(Arc::new(Schema::empty())),
        ))
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut SimpleBlockWriter<'_>,
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

    async fn get_splits(&self, request: &GetSplitsRequest) -> Result<GetSplitsResponse> {
        self.record("doGetSplits", request.catalog_name(), &[addr(request)]);
        Ok(GetSplitsResponse::new(request.catalog_name(), [], None))
    }

    async fn get_data_source_capabilities(
        &self,
        request: &GetDataSourceCapabilitiesRequest,
    ) -> Result<GetDataSourceCapabilitiesResponse> {
        self.record(
            "doGetDataSourceCapabilities",
            request.catalog_name(),
            &[addr(request)],
        );
        Ok(GetDataSourceCapabilitiesResponse::new(
            request.catalog_name(),
            IndexMap::new(),
        ))
    }
}

#[async_trait]
impl MetadataDelegate for RecordingHandler {
    fn partition_schema(&self, catalog_name: &str) -> Result<Schema> {
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
    handler: MultiplexingJdbcMetadataHandler<RecordingHandler>,
    delegate: Arc<RecordingHandler>,
    factory: FakeConnectionFactory,
    allocator: Arc<dyn BlockAllocator>,
}

fn mux(
    factory: &FakeConnectionFactory,
    map: IndexMap<String, Arc<RecordingHandler>>,
) -> Result<MultiplexingJdbcMetadataHandler<RecordingHandler>> {
    MultiplexingJdbcMetadataHandler::new(metadata_context(), Arc::new(factory.clone()), map)
}

fn setup() -> Fixture {
    let delegate = Arc::new(RecordingHandler::new());
    let factory = FakeConnectionFactory::new(FakeConnection::new());
    let mut map: IndexMap<String, Arc<RecordingHandler>> = IndexMap::new();
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
    let request = ListSchemasRequest::new(None, None, FAKE_DATABASE);
    fixture.handler.list_schema_names(&request).await.unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call("doListSchemaNames", FAKE_DATABASE, &[addr(&request)])]
    );
}

#[tokio::test]
async fn do_list_tables() {
    let fixture = setup();
    let request = ListTablesRequest::new(None, None, FAKE_DATABASE, None, None, -1);
    fixture.handler.list_tables(&request).await.unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call("doListTables", FAKE_DATABASE, &[addr(&request)])]
    );
}

#[tokio::test]
async fn do_get_table() {
    let fixture = setup();
    let request = table_request();
    fixture.handler.get_table(&request).await.unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call("doGetTable", FAKE_DATABASE, &[addr(&request)])]
    );
}

#[tokio::test]
async fn do_get_table_layout() {
    let fixture = setup();
    let request = layout_request();
    fixture
        .handler
        .get_table_layout(fixture.allocator.as_ref(), &request)
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
    fixture.handler.partition_schema(FAKE_DATABASE).unwrap();
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
        .partition_schema(UNSUPPORTED_CATALOG)
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
    let context = metadata_context();
    let checker = context.handler().query_status_checker(None, "queryId");
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
    fixture.handler.get_splits(&request).await.unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call("doGetSplits", FAKE_DATABASE, &[addr(&request)])]
    );
}

#[tokio::test]
async fn test_constructor_with_too_many_handlers_should_throw_exception() {
    let fixture = setup();
    let mut map: IndexMap<String, Arc<RecordingHandler>> = IndexMap::new();
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
    let request = table_request();
    fixture
        .handler
        .get_query_passthrough_schema(&request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doGetQueryPassthroughSchema",
            FAKE_DATABASE,
            &[addr(&request)]
        )]
    );
}

#[tokio::test]
async fn test_do_get_data_source_capabilities() {
    let fixture = setup();
    let request = GetDataSourceCapabilitiesRequest::new(None, None, FAKE_DATABASE);
    fixture
        .handler
        .get_data_source_capabilities(&request)
        .await
        .unwrap();
    assert_eq!(
        fixture.delegate.calls(),
        [call(
            "doGetDataSourceCapabilities",
            FAKE_DATABASE,
            &[addr(&request)]
        )]
    );
}

#[tokio::test]
async fn test_close_closes_delegates_and_own_factory() {
    let fixture = setup();
    fixture.handler.close().await.unwrap();
    assert_eq!(fixture.delegate.calls(), [call("close", "", &[])]);
    assert_eq!(fixture.factory.close_calls(), 1);
}

#[tokio::test]
async fn test_close_closes_remaining_delegates_when_one_fails() {
    let failing = Arc::new(RecordingHandler::with_close_error(Some("boom")));
    let healthy = Arc::new(RecordingHandler::new());
    let mut map: IndexMap<String, Arc<RecordingHandler>> = IndexMap::new();
    map.insert("failing".to_string(), failing.clone());
    map.insert("healthy".to_string(), healthy.clone());
    let own_factory = FakeConnectionFactory::new(FakeConnection::new());
    let handler = mux(&own_factory, map).unwrap();

    let error = handler.close().await.unwrap_err();
    assert!(error.is_runtime_exception());
    assert_eq!(error.to_string(), "boom");
    assert_eq!(failing.calls(), [call("close", "", &[])]);
    assert_eq!(healthy.calls(), [call("close", "", &[])]);
    assert_eq!(own_factory.close_calls(), 1);
}
