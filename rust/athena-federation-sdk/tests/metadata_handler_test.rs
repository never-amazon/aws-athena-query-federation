mod handler_support;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, BlockWriter, SchemaBuilder,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::domain::split::Split;
use athena_federation_sdk::domain::table_name::TableName;
use athena_federation_sdk::handlers::{
    FederationRequestHandler, MetadataHandler, MetadataHandlerBase, MetadataHandlerExt,
};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest,
    GetTableRequest, GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederationRequest, FederationResponse, PingRequest};
use athena_federation_sdk::security::{
    CachableSecretsManager, FederatedIdentity, KmsEncryptionProvider, LocalKeyFactory,
};
use athena_federation_sdk::serde::VersionedObjectMapperFactory;
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use handler_support::{Fakes, RecordingAllocator};

const CATALOG: &str = "catalog";
const QUERY_ID: &str = "queryId";
const SCHEMA_NAME: &str = "testSchema";
const TABLE_NAME: &str = "testTable";
const PARTITION_COL: &str = "partition_col";

struct TestHandler {
    base: MetadataHandlerBase,
}

impl FederationRequestHandler for TestHandler {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager {
        self.base.cachable_secrets_manager()
    }

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider {
        self.base.kms_encryption_provider()
    }
}

#[async_trait]
impl MetadataHandler for TestHandler {
    fn metadata_base(&self) -> &MetadataHandlerBase {
        &self.base
    }

    async fn do_list_schema_names(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        Ok(ListSchemasResponse::new(
            "catalog1",
            vec!["schema1".into(), "schema2".into(), "schema3".into()],
        ))
    }

    async fn do_list_tables(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &ListTablesRequest,
    ) -> Result<ListTablesResponse> {
        let schema_name = request.schema_name().unwrap_or_default();
        let tables = vec![
            TableName::new(schema_name, "table1"),
            TableName::new(schema_name, "table2"),
        ];
        Ok(ListTablesResponse::new(
            request.catalog_name(),
            tables,
            None,
        ))
    }

    async fn do_get_table(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        request: &GetTableRequest,
    ) -> Result<GetTableResponse> {
        let schema = SchemaBuilder::new_builder()
            .add_string_field("id")
            .add_int_field("age")
            .build();
        Ok(GetTableResponse::new(
            request.catalog_name(),
            request.table_name().clone(),
            schema,
            indexmap::IndexSet::default(),
        ))
    }

    async fn get_partitions(
        &self,
        block_writer: &mut (dyn BlockWriter + Send),
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
            block.set_value(PARTITION_COL, row_num, "*")?;
            Ok(1)
        })
    }

    async fn do_get_splits(
        &self,
        _allocator: &Arc<dyn BlockAllocator>,
        _request: &GetSplitsRequest,
    ) -> Result<GetSplitsResponse> {
        Err(FederationError::UnsupportedOperation(String::new()))
    }
}

struct Setup {
    handler: TestHandler,
    block_allocator: Arc<dyn BlockAllocator>,
}

fn set_up() -> Setup {
    let config_options = HashMap::from([
        ("spill_bucket".to_string(), "bucket".to_string()),
        ("spill_prefix".to_string(), "prefix".to_string()),
    ]);
    let base = MetadataHandlerBase::builder("test", Fakes::new().clients())
        .config_options(config_options)
        .encryption_key_factory(Some(Arc::new(LocalKeyFactory::new())))
        .spill_bucket(Some("bucket".into()))
        .spill_prefix(Some("prefix".into()))
        .build()
        .unwrap();
    Setup {
        handler: TestHandler { base },
        block_allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

fn layout_request(
    schema: arrow::datatypes::Schema,
    partition_cols: &[&str],
) -> GetTableLayoutRequest {
    GetTableLayoutRequest::new(
        Some(identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        TableName::new(SCHEMA_NAME, TABLE_NAME),
        Constraints::empty(),
        schema,
        partition_cols.iter().copied(),
    )
}

async fn round_trip(setup: &Setup, request: FederationRequest) -> Result<FederationResponse> {
    let object_mapper = VersionedObjectMapperFactory::create(Arc::clone(&setup.block_allocator));
    let input = object_mapper.write_value_as_bytes(&request)?;
    let output = setup.handler.handle_request(&input, None).await?;
    object_mapper.read_response(&output)
}

#[tokio::test]
async fn do_get_table_layout() {
    let s = set_up();
    let request = layout_request(SchemaBuilder::new_builder().build(), &[]);
    let response = s
        .handler
        .do_get_table_layout(&s.block_allocator, &request)
        .await
        .unwrap();
    assert_eq!(1, response.partitions().row_count());
}

#[tokio::test]
async fn do_get_table_layout_with_constraints() {
    let s = set_up();
    let request = layout_request(
        SchemaBuilder::new_builder()
            .add_string_field(PARTITION_COL)
            .build(),
        &[PARTITION_COL],
    );
    let partition_schema = SchemaBuilder::new_builder()
        .add_string_field(PARTITION_COL)
        .build();
    let recording = Arc::new(RecordingAllocator::new());
    let spy_allocator: Arc<dyn BlockAllocator> = recording.clone();
    let response = s
        .handler
        .do_get_table_layout(&spy_allocator, &request)
        .await
        .unwrap();
    assert_eq!(1, response.partitions().row_count());
    assert_eq!(vec![partition_schema], *recording.created.lock().unwrap());
}

#[tokio::test]
async fn test_list_schemas_request() {
    let s = set_up();
    let request = ListSchemasRequest::new(Some(identity()), Some(QUERY_ID.into()), CATALOG);
    let response = round_trip(&s, request.into()).await.unwrap();
    assert!(matches!(response, FederationResponse::ListSchemas(_)));
}

#[tokio::test]
async fn test_list_tables_request() {
    let s = set_up();
    let request = ListTablesRequest::new(
        Some(identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        Some(SCHEMA_NAME.into()),
        None,
        10,
    );
    let response = round_trip(&s, request.into()).await.unwrap();
    assert!(matches!(response, FederationResponse::ListTables(_)));
}

#[tokio::test]
async fn test_get_table_request() {
    let s = set_up();
    let request = GetTableRequest::new(
        Some(identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        TableName::new(SCHEMA_NAME, TABLE_NAME),
        HashMap::new(),
    );
    let response = round_trip(&s, request.into()).await.unwrap();
    assert!(matches!(response, FederationResponse::GetTable(_)));
}

#[tokio::test]
async fn test_get_capabilities_request() {
    let s = set_up();
    let request =
        GetDataSourceCapabilitiesRequest::new(Some(identity()), Some(QUERY_ID.into()), CATALOG);
    let response = round_trip(&s, request.into()).await.unwrap();
    assert!(matches!(
        response,
        FederationResponse::GetDataSourceCapabilities(_)
    ));
}

#[tokio::test]
async fn handle_request() {
    let s = set_up();
    let split = Split::new_builder(
        Some(
            S3SpillLocation::new_builder()
                .with_bucket("bucket")
                .with_split_id("split")
                .build()
                .into(),
        ),
        None,
    )
    .build();
    let invalid_request = ReadRecordsRequest::new(
        None,
        CATALOG,
        QUERY_ID,
        TableName::new(SCHEMA_NAME, TABLE_NAME),
        SchemaBuilder::new_builder().build(),
        split,
        Constraints::empty(),
        1024,
        512,
    );
    let err = round_trip(&s, invalid_request.into()).await.unwrap_err();
    assert!(
        matches!(err, FederationError::AthenaConnector(_)),
        "{err:?}"
    );
    assert!(err
        .to_string()
        .contains("Expected a MetadataRequest but found"));

    let valid_request =
        GetDataSourceCapabilitiesRequest::new(Some(identity()), Some(QUERY_ID.into()), CATALOG);
    let response = round_trip(&s, valid_request.into()).await.unwrap();
    assert!(matches!(
        response,
        FederationResponse::GetDataSourceCapabilities(_)
    ));
}

#[tokio::test]
async fn ping_handle_request() {
    let s = set_up();
    let ping_request = PingRequest::new(Some(identity()), CATALOG, QUERY_ID);
    let response = round_trip(&s, ping_request.into()).await.unwrap();
    assert!(matches!(response, FederationResponse::Ping(_)));
}
