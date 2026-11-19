mod handler_support;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, SchemaBuilder, SimpleBlockWriter,
};
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::domain::{S3SpillLocation, Split, TableName};
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::handlers::{MetadataContext, MetadataHandler, MetadataStreamHandler};
use athena_federation_sdk::lambda::{LambdaError, RequestStreamHandler};
use athena_federation_sdk::metadata::{
    GetDataSourceCapabilitiesRequest, GetSplitsRequest, GetSplitsResponse, GetTableLayoutRequest,
    GetTableRequest, GetTableResponse, ListSchemasRequest, ListSchemasResponse, ListTablesRequest,
    ListTablesResponse,
};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederationRequest, FederationResponse, PingRequest};
use athena_federation_sdk::security::{FederatedIdentity, KeySource};
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use handler_support::{Fakes, RecordingAllocator};

const CATALOG: &str = "catalog";
const QUERY_ID: &str = "queryId";
const SCHEMA_NAME: &str = "testSchema";
const TABLE_NAME: &str = "testTable";
const PARTITION_COL: &str = "partition_col";

struct TestHandler {
    context: MetadataContext,
}

#[async_trait]
impl MetadataHandler for TestHandler {
    fn context(&self) -> &MetadataContext {
        &self.context
    }

    async fn list_schema_names(
        &self,
        _request: &ListSchemasRequest,
    ) -> Result<ListSchemasResponse> {
        Ok(ListSchemasResponse::new(
            "catalog1",
            vec!["schema1".into(), "schema2".into(), "schema3".into()],
        ))
    }

    async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse> {
        let schema_name = request.schema_name().unwrap_or_default();
        if schema_name == "throw_io" {
            return Err(std::io::Error::other("io boom").into());
        }
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

    async fn get_table(&self, request: &GetTableRequest) -> Result<GetTableResponse> {
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
        block_writer: &mut SimpleBlockWriter<'_>,
        _request: &GetTableLayoutRequest,
        _query_status_checker: &QueryStatusChecker,
    ) -> Result<()> {
        block_writer.write_rows(&mut |block: &mut Block, row_num: usize| {
            block.set_value(PARTITION_COL, row_num, "*")?;
            Ok(1)
        })
    }

    async fn get_splits(&self, _request: &GetSplitsRequest) -> Result<GetSplitsResponse> {
        Err(FederationError::UnsupportedOperation(String::new()))
    }
}

struct Setup {
    handler: Arc<TestHandler>,
    block_allocator: Arc<dyn BlockAllocator>,
}

fn set_up() -> Setup {
    let config_options = HashMap::from([
        ("spill_bucket".to_string(), "bucket".to_string()),
        ("spill_prefix".to_string(), "prefix".to_string()),
    ]);
    let context = MetadataContext::builder("test", Fakes::new().clients())
        .config_options(config_options)
        .encryption_key_factory(Some(KeySource::Local))
        .spill_bucket(Some("bucket".into()))
        .spill_prefix(Some("prefix".into()))
        .build()
        .unwrap();
    Setup {
        handler: Arc::new(TestHandler { context }),
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
    let object_mapper =
        ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&setup.block_allocator));
    let input = object_mapper
        .write_request(&request)
        .map(String::into_bytes)?;
    let output = MetadataStreamHandler::new(Arc::clone(&setup.handler))
        .handle_request(&input, None)
        .await?;
    object_mapper.read_response(&output)
}

#[tokio::test]
async fn do_get_table_layout() {
    let s = set_up();
    let request = layout_request(SchemaBuilder::new_builder().build(), &[]);
    let response = s
        .handler
        .get_table_layout(s.block_allocator.as_ref(), &request)
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
        .get_table_layout(spy_allocator.as_ref(), &request)
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
async fn checked_exception_becomes_connector_exception_with_response_and_no_cause() {
    let s = set_up();
    let request = ListTablesRequest::new(
        Some(identity()),
        Some(QUERY_ID.into()),
        CATALOG,
        Some("throw_io".into()),
        None,
        10,
    );
    let err = round_trip(&s, request.into()).await.unwrap_err();
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: io boom\n\
         errorCode: InternalServiceException\n",
        LambdaError::new(&err).to_report()
    );
    let exception = err.as_connector_exception().unwrap();
    assert!(std::error::Error::source(exception).is_none());
    let response = exception.response.as_deref().unwrap();
    assert_eq!("java.io.IOException", response.java_class_name());
}

#[tokio::test]
async fn do_get_table_layout_with_missing_partition_column() {
    let s = set_up();
    let request = layout_request(
        SchemaBuilder::new_builder()
            .add_int_field("c1")
            .add_int_field("p")
            .build(),
        &["zz"],
    );
    let err = s
        .handler
        .get_table_layout(s.block_allocator.as_ref(), &request)
        .await
        .unwrap_err();
    assert_eq!(
        "errorType: java.lang.IllegalArgumentException\n\
         errorMessage: field zz not found in [c1: Int(32, true), p: Int(32, true)]\n",
        LambdaError::new(&err).to_report()
    );
}

#[tokio::test]
async fn java_field_not_found_message_lists_children_and_nullability() {
    let schema = arrow::datatypes::Schema::new(vec![
        arrow::datatypes::Field::new("x", arrow::datatypes::DataType::Int32, false),
        arrow::datatypes::Field::new_list(
            "l",
            arrow::datatypes::Field::new("item", arrow::datatypes::DataType::Utf8, true),
            true,
        ),
    ]);
    let request = layout_request(schema, &["zz"]);
    let s = set_up();
    let err = s
        .handler
        .get_table_layout(s.block_allocator.as_ref(), &request)
        .await
        .unwrap_err();
    assert_eq!(
        "field zz not found in [x: Int(32, true) not null, l: List<item: Utf8>]",
        err.to_string()
    );
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

#[tokio::test]
async fn metadata_handler_runs_as_request_stream_handler() {
    let Setup {
        handler,
        block_allocator,
    } = set_up();
    let stream_handler = MetadataStreamHandler::new(handler);
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&block_allocator));
    let request: FederationRequest =
        GetDataSourceCapabilitiesRequest::new(Some(identity()), Some(QUERY_ID.into()), CATALOG)
            .into();
    let input = object_mapper
        .write_request(&request)
        .map(String::into_bytes)
        .unwrap();
    let output = RequestStreamHandler::handle_request(&stream_handler, &input, None)
        .await
        .unwrap();
    assert!(matches!(
        object_mapper.read_response(&output).unwrap(),
        FederationResponse::GetDataSourceCapabilities(_)
    ));
    let wrong: FederationRequest = ReadRecordsRequest::new(
        Some(identity()),
        CATALOG,
        QUERY_ID,
        TableName::new(SCHEMA_NAME, TABLE_NAME),
        SchemaBuilder::new_builder().build(),
        Split::new_builder(None, None).build(),
        Constraints::empty(),
        100,
        100,
    )
    .into();
    let input = object_mapper
        .write_request(&wrong)
        .map(String::into_bytes)
        .unwrap();
    let err = RequestStreamHandler::handle_request(&stream_handler, &input, None)
        .await
        .unwrap_err();
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: Expected a MetadataRequest but found class com.amazonaws.athena.connector.lambda.records.ReadRecordsRequest\n\
         errorCode: InvalidInputException\n",
        LambdaError::new(&err).to_report()
    );
    assert_runnable(&stream_handler);
}

fn assert_runnable<H: RequestStreamHandler + 'static>(_handler: &H) {}
