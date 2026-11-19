mod handler_support;

use athena_federation_sdk::exceptions::JavaThrowable;
use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::DataType;
use async_trait::async_trait;
use athena_federation_sdk::data::{
    block_utils, BlockAllocator, BlockAllocatorImpl, BlockSpiller, S3BlockSpillReader, S3Client,
    SchemaBuilder,
};
use athena_federation_sdk::domain::predicate::{Constraints, Range, SortedRangeSet, ValueSet};
use athena_federation_sdk::domain::{S3SpillLocation, Split, TableName};
use athena_federation_sdk::handlers::{HandlerContext, RecordHandler, RecordStreamHandler};
use athena_federation_sdk::lambda::{LambdaError, RequestStreamHandler};
use athena_federation_sdk::metadata::{GetDataSourceCapabilitiesRequest, ListSchemasRequest};
use athena_federation_sdk::records::ReadRecordsRequest;
use athena_federation_sdk::request::{FederationRequest, FederationResponse, PingRequest};
use athena_federation_sdk::security::{FederatedIdentity, KeySource};
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
use athena_federation_sdk::{FederationError, QueryStatusChecker, Result};
use handler_support::Fakes;
use indexmap::IndexMap;

const CATALOG: &str = "catalog";
const QUERY_ID: &str = "queryId";

struct TestRecordHandler {
    context: HandlerContext,
}

#[async_trait]
impl RecordHandler for TestRecordHandler {
    fn context(&self) -> &HandlerContext {
        &self.context
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
    fakes: Fakes,
    handler: Arc<TestRecordHandler>,
    block_allocator: Arc<dyn BlockAllocator>,
}

fn set_up() -> Setup {
    let fakes = Fakes::new();
    let config_options = HashMap::from([
        ("spill_bucket".to_string(), "bucket".to_string()),
        ("spill_prefix".to_string(), "prefix".to_string()),
    ]);
    let context = HandlerContext::new("test", fakes.clients(), config_options).unwrap();
    Setup {
        fakes,
        handler: Arc::new(TestRecordHandler { context }),
        block_allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn identity() -> FederatedIdentity {
    FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new())
}

async fn round_trip(setup: &Setup, request: FederationRequest) -> Result<FederationResponse> {
    let object_mapper =
        ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&setup.block_allocator));
    let input = object_mapper
        .write_request(&request)
        .map(String::into_bytes)?;
    let output = RecordStreamHandler::new(Arc::clone(&setup.handler))
        .handle_request(&input, None)
        .await?;
    object_mapper.read_response(&output)
}

#[tokio::test]
async fn handle_request() {
    let s = set_up();
    let invalid_request =
        GetDataSourceCapabilitiesRequest::new(Some(identity()), Some(QUERY_ID.into()), CATALOG);
    let err = round_trip(&s, invalid_request.into()).await.unwrap_err();
    assert!(
        matches!(err, FederationError::AthenaConnector(_)),
        "{err:?}"
    );
    assert!(err
        .to_string()
        .contains("Expected a RecordRequest but found"));
}

#[tokio::test]
async fn handle_request_catch_block() {
    let s = set_up();
    let err = RecordStreamHandler::new(s.handler)
        .handle_request(b"invalid json", None)
        .await
        .unwrap_err();
    let FederationError::RuntimeWrapper { source } = &err else {
        panic!("expected a RuntimeException wrapper, got {err:?}");
    };
    assert_eq!("java.io.IOException", source.java_class_name());
}

#[tokio::test]
async fn do_read_records_spill() {
    let s = set_up();
    let allocator = s.block_allocator.as_ref();
    let mut constraints_map = IndexMap::new();
    constraints_map.insert(
        "c_current_cdemo_sk".to_string(),
        ValueSet::from(
            SortedRangeSet::of_ranges(
                Range::range(
                    allocator,
                    &DataType::Int64,
                    100i64,
                    true,
                    100_000_000i64,
                    true,
                )
                .unwrap(),
                Vec::new(),
            )
            .unwrap(),
        ),
    );
    let spill_location = S3SpillLocation::new_builder()
        .with_bucket(uuid::Uuid::new_v4().to_string())
        .with_split_id(uuid::Uuid::new_v4().to_string())
        .with_query_id(uuid::Uuid::new_v4().to_string())
        .build();
    let split = Split::new_builder(
        Some(spill_location.into()),
        Some(KeySource::Local.create(None).await.unwrap()),
    )
    .add("splitNum", "0")
    .add("totalNumberSplits", "10000")
    .add("scaleFactor", "1")
    .build();
    let request = ReadRecordsRequest::new(
        Some(identity()),
        CATALOG,
        format!("queryId-{}", uuid::Uuid::new_v4()),
        TableName::new("testSchema", "testTable"),
        SchemaBuilder::new_builder().build(),
        split,
        Constraints::new(
            constraints_map,
            Vec::new(),
            Vec::new(),
            -1,
            HashMap::new(),
            None,
        ),
        1_500_000,
        0,
    );

    let raw_response = s
        .handler
        .read_records(&s.block_allocator, &request)
        .await
        .unwrap();
    let FederationResponse::RemoteReadRecords(response) = raw_response else {
        panic!("expected a RemoteReadRecordsResponse, got {raw_response:?}");
    };
    let s3: Arc<dyn S3Client> = Arc::new(s.fakes.s3.clone());
    let spill_reader = S3BlockSpillReader::new(s3, Arc::clone(&s.block_allocator));
    for next in response.remote_blocks() {
        let spill_location = next.as_s3().unwrap();
        let block = spill_reader
            .read_with_schema(
                spill_location,
                response.encryption_key(),
                Arc::new(response.schema().clone()),
            )
            .await
            .unwrap();
        assert!(block_utils::row_to_string(&block, 0).is_ok());
    }
}

#[tokio::test]
async fn ping_handle_request() {
    let s = set_up();
    let ping_request = PingRequest::new(Some(identity()), CATALOG, QUERY_ID);
    let response = round_trip(&s, ping_request.into()).await.unwrap();
    assert!(matches!(response, FederationResponse::Ping(_)));
}

#[tokio::test]
async fn record_handler_runs_as_request_stream_handler() {
    let Setup {
        handler,
        block_allocator,
        ..
    } = set_up();
    let stream_handler = RecordStreamHandler::new(handler);
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&block_allocator));
    let ping: FederationRequest = PingRequest::new(Some(identity()), "ok", "q1").into();
    let input = object_mapper
        .write_request(&ping)
        .map(String::into_bytes)
        .unwrap();
    let output = RequestStreamHandler::handle_request(&stream_handler, &input, None)
        .await
        .unwrap();
    assert!(matches!(
        object_mapper.read_response(&output).unwrap(),
        FederationResponse::Ping(_)
    ));
    let wrong: FederationRequest =
        ListSchemasRequest::new(Some(identity()), Some("q1".into()), "ok").into();
    let input = object_mapper
        .write_request(&wrong)
        .map(String::into_bytes)
        .unwrap();
    let err = RequestStreamHandler::handle_request(&stream_handler, &input, None)
        .await
        .unwrap_err();
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: Expected a RecordRequest but found class com.amazonaws.athena.connector.lambda.metadata.ListSchemasRequest\n\
         errorCode: InvalidInputException\n",
        LambdaError::new(&err).to_report()
    );
    assert_runnable(&stream_handler);
}

fn assert_runnable<H: RequestStreamHandler + 'static>(_handler: &H) {}
