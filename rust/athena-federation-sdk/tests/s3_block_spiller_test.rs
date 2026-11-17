use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::spill_config::SpillConfig;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, BlockSpiller, BlockUtils, GetObjectRequest, PutObjectRequest,
    S3BlockSpiller, S3Client, SchemaBuilder, Value,
};
use athena_federation_sdk::domain::predicate::ConstraintEvaluator;
use athena_federation_sdk::domain::spill::{S3SpillLocation, SpillLocation};
use athena_federation_sdk::exceptions::FederationError;
use athena_federation_sdk::security::{AwsServiceError, EncryptionKeyFactory, LocalKeyFactory};

const BUCKET: &str = "MyBucket";
const PREFIX: &str = "blocks/spill";
const REQUEST_ID: &str = "requestId";
const SPLIT_ID: &str = "splitId";

#[derive(Default)]
struct MockS3 {
    put_error: Mutex<Option<AwsServiceError>>,
    put_requests: Mutex<Vec<PutObjectRequest>>,
    bytes: Mutex<Option<Vec<u8>>>,
    get_requests: Mutex<Vec<GetObjectRequest>>,
}

impl MockS3 {
    fn reset(&self) {
        self.put_requests.lock().unwrap().clear();
        self.get_requests.lock().unwrap().clear();
        *self.put_error.lock().unwrap() = None;
    }

    fn put_count(&self) -> usize {
        self.put_requests.lock().unwrap().len()
    }

    fn last_put(&self) -> PutObjectRequest {
        self.put_requests.lock().unwrap().last().cloned().unwrap()
    }
}

#[async_trait]
impl S3Client for MockS3 {
    async fn put_object(&self, request: PutObjectRequest, body: Vec<u8>) -> Result<(), AwsServiceError> {
        self.put_requests.lock().unwrap().push(request);
        if let Some(e) = self.put_error.lock().unwrap().clone() {
            return Err(e);
        }
        *self.bytes.lock().unwrap() = Some(body);
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        self.get_requests.lock().unwrap().push(request);
        Ok(self.bytes.lock().unwrap().clone().unwrap_or_default())
    }
}

struct Setup {
    mock_s3: Arc<MockS3>,
    allocator: Arc<BlockAllocatorImpl>,
    spill_config: SpillConfig,
    block_writer: S3BlockSpiller,
    expected: Block,
}

impl Setup {
    async fn tear_down(self) {
        self.allocator.close();
        self.block_writer.close().await;
    }

    fn create_block_spiller(&self, config_options: HashMap<String, String>) -> S3BlockSpiller {
        S3BlockSpiller::new(
            self.mock_s3.clone(),
            self.spill_config.clone(),
            self.allocator.clone(),
            self.expected.get_schema().clone(),
            Arc::new(ConstraintEvaluator::empty_evaluator()),
            config_options,
        )
        .unwrap()
    }

    async fn execute_spill_with_config(&self, config_options: HashMap<String, String>) -> PutObjectRequest {
        let block_writer = self.create_block_spiller(config_options);
        block_writer.write(&self.expected).await.unwrap();
        assert_eq!(1, self.mock_s3.put_count());
        let captured_request = self.mock_s3.last_put();
        block_writer.close().await;
        captured_request
    }
}

fn spill_location() -> SpillLocation {
    SpillLocation::S3(
        S3SpillLocation::new_builder()
            .with_bucket(BUCKET)
            .with_prefix(PREFIX)
            .with_query_id(REQUEST_ID)
            .with_split_id(SPLIT_ID)
            .with_is_directory(true)
            .build(),
    )
}

fn set(block: &mut Block, field: &str, row: usize, value: impl Into<Value>) {
    BlockUtils::set_value(block.get_field_vector_mut(field).unwrap(), row, &value.into()).unwrap();
}

fn two_column_schema() -> Arc<Schema> {
    Arc::new(
        SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", DataType::Utf8)
            .build(),
    )
}

fn one_column_schema() -> Arc<Schema> {
    Arc::new(SchemaBuilder::new_builder().add_field_with_type("col1", DataType::Int32).build())
}

async fn setup() -> Setup {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let mock_s3 = Arc::new(MockS3::default());
    let schema = two_column_schema();
    let spill_config = SpillConfig::new_builder()
        .with_encryption_key(Some(LocalKeyFactory::new().create().await.unwrap()))
        .with_request_id(REQUEST_ID)
        .with_spill_location(spill_location())
        .with_request_id(REQUEST_ID)
        .build()
        .unwrap();
    let block_writer = S3BlockSpiller::new(
        mock_s3.clone(),
        spill_config.clone(),
        allocator.clone(),
        schema.clone(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();
    let mut expected = allocator.create_block(schema);
    set(&mut expected, "col1", 1, 100);
    set(&mut expected, "col2", 1, "VarChar");
    set(&mut expected, "col1", 1, 101);
    set(&mut expected, "col2", 1, "VarChar1");
    expected.set_row_count(2);
    Setup {
        mock_s3,
        allocator,
        spill_config,
        block_writer,
        expected,
    }
}

async fn async_spill_config(max_block_bytes: i64, num_spill_threads: i32) -> SpillConfig {
    SpillConfig::new_builder()
        .with_encryption_key(Some(LocalKeyFactory::new().create().await.unwrap()))
        .with_request_id(REQUEST_ID)
        .with_spill_location(spill_location())
        .with_request_id(REQUEST_ID)
        .with_max_block_bytes(max_block_bytes)
        .with_max_inline_block_bytes(0)
        .with_num_spill_threads(num_spill_threads)
        .build()
        .unwrap()
}

fn header(request: &PutObjectRequest, name: &str) -> Option<String> {
    request
        .override_configuration()
        .and_then(|o| o.headers().get(name))
        .and_then(|v| v.first().cloned())
}

#[tokio::test]
async fn spill_test() {
    let s = setup().await;
    let key0 = format!("{}/{}/{}.0", PREFIX, REQUEST_ID, SPLIT_ID);
    let key1 = format!("{}/{}/{}.1", PREFIX, REQUEST_ID, SPLIT_ID);

    let block_location = s.block_writer.write(&s.expected).await.unwrap();
    let SpillLocation::S3(block_location) = block_location;
    assert_eq!(BUCKET, block_location.get_bucket());
    assert_eq!(key0, block_location.get_key());
    assert_eq!(1, s.mock_s3.put_count());
    assert_eq!(BUCKET, s.mock_s3.last_put().bucket());
    assert_eq!(key0, s.mock_s3.last_put().key());

    let block_location2 = s.block_writer.write(&s.expected).await.unwrap();
    let SpillLocation::S3(block_location2) = block_location2;
    assert_eq!(BUCKET, block_location2.get_bucket());
    assert_eq!(key1, block_location2.get_key());
    assert_eq!(2, s.mock_s3.put_count());
    assert_eq!(BUCKET, s.mock_s3.last_put().bucket());
    assert_eq!(key1, s.mock_s3.last_put().key());
    assert_eq!(0, s.mock_s3.get_requests.lock().unwrap().len());

    s.mock_s3.reset();
    let block = s
        .block_writer
        .read(&block_location2, s.spill_config.get_encryption_key(), s.expected.get_schema().clone())
        .await
        .unwrap();
    assert_eq!(s.expected, block);
    assert_eq!(1, s.mock_s3.get_requests.lock().unwrap().len());
    assert_eq!(0, s.mock_s3.put_count());
    s.tear_down().await;
}

#[tokio::test]
async fn spill_test_with_request_headers_sets_headers_in_override_configuration() {
    let s = setup().await;
    let spill_headers = "{\"x-amz-server-side-encryption\":\"aws:kms\",\"x-amz-server-side-encryption-aws-kms-key-id\":\"arn:aws:kms:us-east-1:123456789012:key/test-key-id\"}";
    let config_options = HashMap::from([("spill_put_request_headers".to_string(), spill_headers.to_string())]);

    let captured_request = s.execute_spill_with_config(config_options).await;

    assert!(captured_request.override_configuration().is_some(), "Request should have overrideConfiguration");
    assert_eq!(Some("aws:kms".to_string()), header(&captured_request, "x-amz-server-side-encryption"));
    assert_eq!(
        Some("arn:aws:kms:us-east-1:123456789012:key/test-key-id".to_string()),
        header(&captured_request, "x-amz-server-side-encryption-aws-kms-key-id")
    );
    assert!(captured_request.metadata().is_empty(), "Metadata should be null or empty, not contain headers");
    s.tear_down().await;
}

#[tokio::test]
async fn spill_test_without_request_headers_does_not_set_override_configuration() {
    let s = setup().await;
    let captured_request = s.execute_spill_with_config(HashMap::new()).await;
    assert!(
        captured_request.override_configuration().is_none(),
        "Request should not have overrideConfiguration when no headers configured"
    );
    assert!(captured_request.metadata().is_empty(), "Metadata should be null when no headers configured");
    s.tear_down().await;
}

#[tokio::test]
async fn spill_test_with_invalid_json_headers_handles_gracefully() {
    let s = setup().await;
    let invalid_json = "{\"x-amz-server-side-encryption\":\"aws:kms\"invalid}";
    let config_options = HashMap::from([("spill_put_request_headers".to_string(), invalid_json.to_string())]);
    let captured_request = s.execute_spill_with_config(config_options).await;
    assert!(
        captured_request.override_configuration().is_none(),
        "Request should not have overrideConfiguration when JSON is invalid"
    );
    s.tear_down().await;
}

#[tokio::test]
async fn spill_test_with_multiple_headers_sets_all_headers_in_override_configuration() {
    let s = setup().await;
    let spill_headers = "{\"x-amz-server-side-encryption\":\"aws:kms\",\"x-amz-server-side-encryption-aws-kms-key-id\":\"arn:aws:kms:us-east-1:123456789012:key/test-key-id\",\"x-amz-storage-class\":\"STANDARD_IA\"}";
    let config_options = HashMap::from([("spill_put_request_headers".to_string(), spill_headers.to_string())]);

    let captured_request = s.execute_spill_with_config(config_options).await;

    let override_config = captured_request
        .override_configuration()
        .expect("Request should have overrideConfiguration");
    assert_eq!(Some("aws:kms".to_string()), header(&captured_request, "x-amz-server-side-encryption"));
    assert_eq!(
        Some("arn:aws:kms:us-east-1:123456789012:key/test-key-id".to_string()),
        header(&captured_request, "x-amz-server-side-encryption-aws-kms-key-id")
    );
    assert_eq!(Some("STANDARD_IA".to_string()), header(&captured_request, "x-amz-storage-class"));
    assert_eq!(3, override_config.headers().len(), "Should have 3 headers");
    assert!(captured_request.metadata().is_empty(), "Metadata should be null, not contain headers");
    s.tear_down().await;
}

#[tokio::test]
async fn get_spill_locations_throws_on_async_s3_error() {
    let s = setup().await;
    let async_spiller = S3BlockSpiller::new(
        s.mock_s3.clone(),
        async_spill_config(100, 1).await,
        s.allocator.clone(),
        one_column_schema(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();
    *s.mock_s3.put_error.lock().unwrap() = Some(AwsServiceError::new("Access Denied"));

    let result: Result<Vec<SpillLocation>, FederationError> = async {
        async_spiller
            .write_rows(&mut |block: &mut Block, row_num: usize| {
                set(block, "col1", row_num, 100);
                Ok(1)
            })
            .await?;
        async_spiller.get_spill_locations().await
    }
    .await;
    async_spiller.close().await;

    let ex = result.expect_err("expected the async S3 error");
    assert!(ex.is_runtime_exception());
    s.tear_down().await;
}

#[tokio::test]
async fn get_spill_locations_rethrows_sync_final_spill_error() {
    let s = setup().await;
    let sync_spill_config = SpillConfig::new_builder()
        .with_encryption_key(Some(LocalKeyFactory::new().create().await.unwrap()))
        .with_request_id(REQUEST_ID)
        .with_spill_location(spill_location())
        .with_max_block_bytes(1_000_000)
        .with_max_inline_block_bytes(0)
        .with_num_spill_threads(0)
        .build()
        .unwrap();
    let sync_spiller = S3BlockSpiller::new(
        s.mock_s3.clone(),
        sync_spill_config,
        s.allocator.clone(),
        one_column_schema(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();
    let s3_error = AwsServiceError::new("The specified bucket is not valid.");
    *s.mock_s3.put_error.lock().unwrap() = Some(s3_error.clone());

    let result: Result<Vec<SpillLocation>, FederationError> = async {
        sync_spiller
            .write_rows(&mut |block: &mut Block, row_num: usize| {
                set(block, "col1", row_num, 100);
                Ok(1)
            })
            .await?;
        sync_spiller.get_spill_locations().await
    }
    .await;
    sync_spiller.close().await;

    let ex = result.expect_err("expected the S3 error");
    assert!(matches!(&ex, FederationError::AwsService(e) if e == &s3_error));
    assert_eq!(1, s.mock_s3.put_count());
    s.tear_down().await;
}

#[tokio::test]
async fn spilled_throws_on_async_s3_error() {
    let s = setup().await;
    let async_spiller = S3BlockSpiller::new(
        s.mock_s3.clone(),
        async_spill_config(100, 1).await,
        s.allocator.clone(),
        two_column_schema(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();
    *s.mock_s3.put_error.lock().unwrap() = Some(AwsServiceError::new("Access Denied"));

    for _ in 0..20 {
        async_spiller
            .write_rows(&mut |block: &mut Block, row_num: usize| {
                set(block, "col1", row_num, 100);
                set(block, "col2", row_num, "SomeDataToIncreaseBlockSize");
                Ok(1)
            })
            .await
            .unwrap();
    }
    async_spiller.close().await;

    let ex = async_spiller.spilled().await.expect_err("spilled() should throw");
    assert!(ex.is_runtime_exception());
    assert!(s.mock_s3.put_count() > 0);
    s.tear_down().await;
}

#[tokio::test]
async fn get_spill_locations_succeeds_when_s3_write_succeeds() {
    let s = setup().await;
    let async_spiller = S3BlockSpiller::new(
        s.mock_s3.clone(),
        async_spill_config(100, 1).await,
        s.allocator.clone(),
        one_column_schema(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();

    async_spiller
        .write_rows(&mut |block: &mut Block, row_num: usize| {
            set(block, "col1", row_num, 100);
            Ok(1)
        })
        .await
        .unwrap();
    let locations = async_spiller.get_spill_locations().await.unwrap();
    async_spiller.close().await;

    assert!(!locations.is_empty());
    s.tear_down().await;
}

#[derive(Default)]
struct KeyedS3 {
    objects: Mutex<HashMap<String, Vec<u8>>>,
}

#[async_trait]
impl S3Client for KeyedS3 {
    async fn put_object(&self, request: PutObjectRequest, body: Vec<u8>) -> Result<(), AwsServiceError> {
        tokio::task::yield_now().await;
        self.objects.lock().unwrap().insert(request.key().to_string(), body);
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        Ok(self.objects.lock().unwrap().get(request.key()).cloned().unwrap_or_default())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn async_spill_numbers_follow_block_order_on_multi_thread_runtime() {
    const BLOCKS: i32 = 200;
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let store = Arc::new(KeyedS3::default());
    let spill_config = async_spill_config(1, 4).await;
    let encryption_key = spill_config.get_encryption_key().cloned();
    let spiller = S3BlockSpiller::new(
        store.clone(),
        spill_config,
        allocator.clone(),
        one_column_schema(),
        Arc::new(ConstraintEvaluator::empty_evaluator()),
        HashMap::new(),
    )
    .unwrap();

    for i in 0..BLOCKS {
        spiller
            .write_rows(&mut |block: &mut Block, row_num: usize| {
                set(block, "col1", row_num, i);
                Ok(1)
            })
            .await
            .unwrap();
    }
    let locations = spiller.get_spill_locations().await.unwrap();
    assert_eq!(BLOCKS as usize, locations.len());

    let base_key = match spill_location() {
        SpillLocation::S3(l) => l.get_key().to_string(),
    };
    let mut seen = Vec::new();
    for location in &locations {
        let SpillLocation::S3(location) = location;
        let number: i32 = location
            .get_key()
            .strip_prefix(&format!("{}.", base_key))
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("unexpected key {}", location.get_key()));
        let block = spiller.read(location, encryption_key.as_ref(), one_column_schema()).await.unwrap();
        let expected = BlockUtils::new_block(allocator.as_ref(), "col1", &DataType::Int32, vec![number]).unwrap();
        assert_eq!(
            BlockUtils::row_to_string(&expected, 0).unwrap(),
            BlockUtils::row_to_string(&block, 0).unwrap(),
            "block spilled to {} is not block {}",
            location.get_key(),
            number
        );
        seen.push(number);
    }
    seen.sort_unstable();
    assert_eq!((0..BLOCKS).collect::<Vec<_>>(), seen);

    spiller.close().await;
    allocator.close();
}
