mod handler_support;

use athena_federation_sdk::data::block_utils;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::DataType;
use async_trait::async_trait;
use athena_federation_sdk::data::{
    BlockAllocator, BlockAllocatorImpl, BlockSpiller, SchemaBuilder, SimpleBlockWriter,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::handlers::{
    CompositeHandler, FederationRequestHandler, MetadataHandler, MetadataHandlerBase,
    RecordHandler, RecordHandlerBase,
};
use athena_federation_sdk::metadata::{
    GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest, GetTableLayoutResponse,
    GetTableRequest, GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::records::{ReadRecordsRequest, ReadRecordsResponse};
use athena_federation_sdk::request::{
    FederationRequest, FederationResponse, PingRequest, PingResponse,
};
use athena_federation_sdk::security::{
    AwsService, AwsServiceError, CachableSecretsManager, FederatedIdentity, KmsEncryptionProvider,
};
use athena_federation_sdk::serde::{object_mapper_factory, ObjectMapper};
use athena_federation_sdk::udf::{UserDefinedFunctionRequest, UserDefinedFunctionType};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use handler_support::Fakes;

#[derive(Default)]
struct Calls {
    list_schemas: AtomicUsize,
    list_tables: AtomicUsize,
    get_table: AtomicUsize,
    get_table_layout: AtomicUsize,
    get_splits: AtomicUsize,
    ping: AtomicUsize,
    read_records: AtomicUsize,
    list_tables_requests: Mutex<Vec<ListTablesRequest>>,
}

struct MockMetadataHandler {
    base: MetadataHandlerBase,
    calls: Arc<Calls>,
    allocator: Arc<dyn BlockAllocator>,
}

impl FederationRequestHandler for MockMetadataHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.kms_encryption_provider()
    }
}

#[async_trait]
impl MetadataHandler for MockMetadataHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        &self.base
    }

    async fn do_list_schema_names(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        self.calls.list_schemas.fetch_add(1, Ordering::SeqCst);
        Ok(ListSchemasResponse::new("catalog", vec!["schema1".into()]))
    }

    async fn do_list_tables(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        self.calls.list_tables.fetch_add(1, Ordering::SeqCst);
        self.calls
            .list_tables_requests
            .lock()
            .unwrap()
            .push(request.clone());
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            vec![TableName::new("testSchema", "testTable")],
            None,
        ))
    }

    async fn do_get_table(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        self.calls.get_table.fetch_add(1, Ordering::SeqCst);
        Ok(GetTableResponse::new(
            "catalog",
            TableName::new("schema", "table"),
            SchemaBuilder::new_builder()
                .add_string_field("col1")
                .build(),
            indexmap::IndexSet::default(),
        ))
    }

    async fn do_get_table_layout(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &GetTableLayoutRequest,
    ) -> Result<GetTableLayoutResponse> {
        self.calls.get_table_layout.fetch_add(1, Ordering::SeqCst);
        Ok(GetTableLayoutResponse::new(
            "catalog",
            TableName::new("schema", "table"),
            block_utils::new_block(self.allocator.as_ref(), "col1", &DataType::Int64, [1i64])?,
        ))
    }

    async fn get_partitions(
        &self,
        _block_writer: &mut SimpleBlockWriter<'_>,
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Ok(())
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        self.calls.get_splits.fetch_add(1, Ordering::SeqCst);
        Ok(GetSplitsResponse::new(
            "catalog",
            [Split::new_builder(None, None).build()],
            None,
        ))
    }

    async fn do_ping(&self, _request: &PingRequest) -> PingResponse {
        self.calls.ping.fetch_add(1, Ordering::SeqCst);
        PingResponse::new("catalog", "queryId", Some("type".into()), 23, 2)
    }
}

struct MockRecordHandler {
    base: RecordHandlerBase,
    calls: Arc<Calls>,
    allocator: Arc<dyn BlockAllocator>,
}

impl FederationRequestHandler for MockRecordHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.kms_encryption_provider()
    }
}

#[async_trait]
impl RecordHandler for MockRecordHandler {
    fn record_base(&self) -> &RecordHandlerBase {
        &self.base
    }

    async fn do_read_records(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &ReadRecordsRequest,
    ) -> Result<FederationResponse> {
        self.calls.read_records.fetch_add(1, Ordering::SeqCst);
        Ok(ReadRecordsResponse::new(
            "catalog",
            block_utils::new_empty_block(self.allocator.as_ref(), "col", &DataType::Int32),
        )
        .into())
    }

    async fn read_with_constraint(
        &self,
        _spiller: &dyn BlockSpiller,
        _records_request: &ReadRecordsRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        Ok(())
    }
}

struct Setup {
    calls: Arc<Calls>,
    composite_handler: CompositeHandler,
    allocator: Arc<dyn BlockAllocator>,
    object_mapper: ObjectMapper,
}

fn set_up() -> Setup {
    let fakes = Fakes::new();
    let allocator: Arc<dyn BlockAllocator> = Arc::new(BlockAllocatorImpl::new());
    let calls = Arc::new(Calls::default());
    let metadata = MockMetadataHandler {
        base: MetadataHandlerBase::builder("type", fakes.clients())
            .build()
            .unwrap(),
        calls: calls.clone(),
        allocator: allocator.clone(),
    };
    let record = MockRecordHandler {
        base: RecordHandlerBase::new("type", fakes.clients(), HashMap::new()).unwrap(),
        calls: calls.clone(),
        allocator: allocator.clone(),
    };
    Setup {
        calls,
        composite_handler: CompositeHandler::new(Arc::new(metadata), Arc::new(record)),
        object_mapper: object_mapper_factory::create(allocator.clone()),
        allocator,
    }
}

fn fake_identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

async fn handle(s: &Setup, request: FederationRequest) -> Result<Vec<u8>> {
    s.composite_handler
        .handle_request_with_object_mapper(&s.allocator, &request, &s.object_mapper)
        .await
}

#[tokio::test]
async fn do_read_records() {
    let s = set_up();
    let schema_for_read = SchemaBuilder::new_builder()
        .add_field_with_type("col1", DataType::Int32)
        .build();
    let req = ReadRecordsRequest::new(
        Some(fake_identity()),
        "catalog",
        "queryId-0",
        TableName::new("schema", "table"),
        schema_for_read,
        Split::new_builder(
            Some(
                S3SpillLocation::new_builder()
                    .with_bucket("athena-virtuoso-test")
                    .with_prefix("lambda-spill")
                    .with_query_id(uuid::Uuid::new_v4().to_string())
                    .with_split_id(uuid::Uuid::new_v4().to_string())
                    .with_is_directory(true)
                    .build()
                    .into(),
            ),
            None,
        )
        .build(),
        Constraints::empty(),
        100_000_000_000,
        100_000_000_000,
    );
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.read_records.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_list_schema_names() {
    let s = set_up();
    let req = ListSchemasRequest::new(None, None, "catalog");
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.list_schemas.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_list_tables() {
    let s = set_up();
    let req = ListTablesRequest::new(None, None, "catalog", None, None, -1);
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.list_tables.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_get_table() {
    let s = set_up();
    let req = GetTableRequest::new(
        None,
        None,
        "catalog",
        TableName::new("schema", "table"),
        HashMap::new(),
    );
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.get_table.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_get_table_layout() {
    let s = set_up();
    let req = GetTableLayoutRequest::new(
        None,
        None,
        "catalog",
        TableName::new("schema", "table"),
        Constraints::empty(),
        SchemaBuilder::new_builder().build(),
        Vec::<String>::new(),
    );
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.get_table_layout.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_get_splits() {
    let s = set_up();
    let partitions = block_utils::new_empty_block(s.allocator.as_ref(), "p", &DataType::Int32);
    let req = GetSplitsRequest::new(
        Some(fake_identity()),
        None,
        "catalog",
        TableName::new("schema", "table"),
        partitions,
        Vec::new(),
        Constraints::empty(),
        None,
    );
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.get_splits.load(Ordering::SeqCst));
}

#[tokio::test]
async fn do_ping() {
    let s = set_up();
    let req = PingRequest::new(None, "catalog", "queryId");
    handle(&s, req.into()).await.unwrap();
    assert_eq!(1, s.calls.ping.load(Ordering::SeqCst));
}

#[tokio::test]
async fn handle_request_with_valid_input() {
    let s = set_up();
    let request_data = r#"{
          "@type": "ListTablesRequest",
          "identity": {
            "id": "UNKNOWN",
            "principal": "UNKNOWN",
            "account": "12345678910",
            "arn": "arn:aws:iam::12345678910:user/test@test.com",
            "tags": {},
            "groups": []
          },
          "queryId": "auto-generated-by-athena",
          "catalogName": "testCatalog",
          "schemaName": "testSchema",
          "nextToken": "testTable",
          "pageSize": 5
        }"#;
    let identity = FederatedIdentity::new(
        "arn:aws:iam::12345678910:user/test@test.com",
        "12345678910",
        HashMap::new(),
        Vec::new(),
        HashMap::new(),
    );
    let expected_request = ListTablesRequest::new(
        Some(identity),
        Some("auto-generated-by-athena".into()),
        "testCatalog",
        Some("testSchema".into()),
        Some("testTable".into()),
        5,
    );
    let output = s
        .composite_handler
        .handle_request(request_data.as_bytes(), None)
        .await
        .unwrap();
    assert_eq!(1, s.calls.list_tables.load(Ordering::SeqCst));
    assert_eq!(
        vec![expected_request],
        *s.calls.list_tables_requests.lock().unwrap()
    );
    let output_data = String::from_utf8(output).unwrap();
    assert!(
        output_data.contains(r#""tables":[{"schemaName":"testSchema","tableName":"testTable"}]"#)
    );
}

#[tokio::test]
async fn handle_request_with_invalid_input() {
    let s = set_up();
    let err = s
        .composite_handler
        .handle_request(br#"{ "invalid": "data" }"#, None)
        .await
        .unwrap_err();
    let exception_message = match &err {
        FederationError::RuntimeWrapper { source } => source.to_string(),
        other => other.to_string(),
    };
    assert!(
        exception_message.contains("Could not resolve subtype of"),
        "{exception_message}"
    );
}

#[tokio::test]
async fn handle_request_with_unknown_request_throws_invalid_input_exception() {
    let s = set_up();
    let block = block_utils::new_empty_block(s.allocator.as_ref(), "col", &DataType::Int32);
    let unknown_request = UserDefinedFunctionRequest::new(
        None,
        block,
        SchemaBuilder::new_builder().add_int_field("out").build(),
        "method",
        UserDefinedFunctionType::SCALAR,
    );
    let err = handle(&s, unknown_request.into()).await.unwrap_err();
    assert!(err.to_string().contains("Unknown request class"));
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        err.error_code()
    );
}

#[test]
fn test_handle_exception_kms_exception_transformed_to_athena_connector_exception() {
    let s = set_up();
    let kms_exception = FederationError::AwsService(AwsServiceError::service(
        AwsService::Kms,
        None,
        Some("KMS access denied".into()),
    ));
    let result = s.composite_handler.handle_exception(kms_exception);
    assert!(matches!(result, FederationError::AthenaConnector(_)));
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        result.error_code()
    );
}

#[test]
fn test_handle_exception_non_kms_exception_not_transformed() {
    let s = set_up();
    let runtime_exception = FederationError::Runtime("Generic error".into());
    let result = s.composite_handler.handle_exception(runtime_exception);
    assert!(matches!(&result, FederationError::Runtime(m) if m == "Generic error"));
}

#[test]
fn test_map_http_error_code_mapping() {
    let data = [
        (400, FederationSourceErrorCode::InvalidInputException),
        (401, FederationSourceErrorCode::InvalidCredentialsException),
        (403, FederationSourceErrorCode::AccessDeniedException),
        (404, FederationSourceErrorCode::EntityNotFoundException),
        (408, FederationSourceErrorCode::OperationTimeoutException),
        (409, FederationSourceErrorCode::InvalidInputException),
        (429, FederationSourceErrorCode::ThrottlingException),
        (500, FederationSourceErrorCode::InternalServiceException),
        (
            501,
            FederationSourceErrorCode::OperationNotSupportedException,
        ),
        (502, FederationSourceErrorCode::InternalServiceException),
        (503, FederationSourceErrorCode::InternalServiceException),
        (504, FederationSourceErrorCode::OperationTimeoutException),
        (0, FederationSourceErrorCode::InvalidInputException),
        (999, FederationSourceErrorCode::InvalidInputException),
    ];
    for (http_code, expected_error_code) in data {
        assert_eq!(
            expected_error_code,
            CompositeHandler::map_http_error_code(http_code),
            "HTTP {http_code} should map to {expected_error_code:?}"
        );
    }
}
