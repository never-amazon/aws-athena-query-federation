use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Schema};
use async_trait::async_trait;
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, BlockUtils, GetObjectRequest, PutObjectRequest,
    S3BlockSpillReader, S3Client, SchemaBuilder, Value,
};
use athena_federation_sdk::domain::spill::S3SpillLocation;
use athena_federation_sdk::security::{
    AesGcmBlockCrypto, AwsService, AwsServiceError, BlockCrypto, EncryptionKey,
    EncryptionKeyFactory, LocalKeyFactory, NoOpBlockCrypto,
};
use athena_federation_sdk::FederationError;

const BUCKET: &str = "test-bucket";
const PREFIX: &str = "test-prefix";
const REQUEST_ID: &str = "1234";
const SPLIT_ID: &str = "5678";

#[derive(Default)]
struct MockS3 {
    response: Mutex<Option<Result<Vec<u8>, AwsServiceError>>>,
    get_requests: Mutex<Vec<GetObjectRequest>>,
    put_requests: Mutex<usize>,
}

impl MockS3 {
    fn when_get_object(&self, response: Result<Vec<u8>, AwsServiceError>) {
        *self.response.lock().unwrap() = Some(response);
    }

    fn count(&self) -> usize {
        self.get_requests.lock().unwrap().len()
    }
}

#[async_trait]
impl S3Client for MockS3 {
    async fn put_object(
        &self,
        _request: PutObjectRequest,
        _body: Vec<u8>,
    ) -> Result<(), AwsServiceError> {
        *self.put_requests.lock().unwrap() += 1;
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        self.get_requests.lock().unwrap().push(request);
        self.response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Ok(Vec::new()))
    }
}

struct Setup {
    allocator: Arc<BlockAllocatorImpl>,
    schema: Arc<Schema>,
    encryption_key: EncryptionKey,
    mock_s3: Arc<MockS3>,
    block_reader: S3BlockSpillReader,
    spill_location: S3SpillLocation,
    expected: Block,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.allocator.close();
    }
}

fn set(block: &mut Block, field: &str, row: usize, value: impl Into<Value>) {
    BlockUtils::set_value(
        block.get_field_vector_mut(field).unwrap(),
        row,
        &value.into(),
    )
    .unwrap();
}

async fn setup() -> Setup {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let mock_s3 = Arc::new(MockS3::default());
    let block_reader = S3BlockSpillReader::new(mock_s3.clone(), allocator.clone());
    let schema = Arc::new(
        SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", DataType::Utf8)
            .build(),
    );
    let encryption_key = LocalKeyFactory::new().create().await.unwrap();
    let spill_location = S3SpillLocation::new_builder()
        .with_bucket(BUCKET)
        .with_prefix(PREFIX)
        .with_query_id(REQUEST_ID)
        .with_split_id(SPLIT_ID)
        .with_is_directory(false)
        .build();
    let mut expected = allocator.create_block(schema.clone());
    set(&mut expected, "col1", 0, 100);
    set(&mut expected, "col2", 0, "VarChar");
    set(&mut expected, "col1", 1, 101);
    set(&mut expected, "col2", 1, "VarChar1");
    expected.set_row_count(2);
    Setup {
        allocator,
        schema,
        encryption_key,
        mock_s3,
        block_reader,
        spill_location,
        expected,
    }
}

fn assert_connector_exception(e: &FederationError) {
    assert!(
        e.as_connector_exception().is_some(),
        "expected AthenaConnectorException, got {e:?}"
    );
}

#[tokio::test]
async fn read_encrypted_block_succeeds() {
    let s = setup().await;
    let block_crypto = AesGcmBlockCrypto::new(s.allocator.clone());
    let spilled_bytes = block_crypto
        .encrypt(Some(&s.encryption_key), &s.expected)
        .unwrap();
    s.mock_s3.when_get_object(Ok(spilled_bytes));

    let actual = s
        .block_reader
        .read_with_schema(&s.spill_location, Some(&s.encryption_key), s.schema.clone())
        .await
        .unwrap();

    assert_eq!(s.expected, actual);
    assert_eq!(1, s.mock_s3.count());
    assert_eq!(0, *s.mock_s3.put_requests.lock().unwrap());
    assert_eq!(
        GetObjectRequest::new(BUCKET, format!("{PREFIX}/{REQUEST_ID}/{SPLIT_ID}")),
        s.mock_s3.get_requests.lock().unwrap()[0]
    );
}

#[tokio::test]
async fn read_bytes_encrypted_bytes_succeeds() {
    let s = setup().await;
    let block_crypto = AesGcmBlockCrypto::new(s.allocator.clone());
    let encrypted_bytes = block_crypto
        .encrypt(Some(&s.encryption_key), &s.expected)
        .unwrap();
    s.mock_s3.when_get_object(Ok(encrypted_bytes));

    let bytes = s
        .block_reader
        .read(&s.spill_location, Some(&s.encryption_key))
        .await
        .unwrap();

    assert!(!bytes.is_empty());
    assert_eq!(1, s.mock_s3.count());
    assert_eq!(0, *s.mock_s3.put_requests.lock().unwrap());
}

#[tokio::test]
async fn read_null_spill_location_throws_null_pointer_exception() {
    let s = setup().await;
    let ex = s
        .block_reader
        .read_with_schema(None, Some(&s.encryption_key), s.schema.clone())
        .await
        .unwrap_err();
    assert!(matches!(ex, FederationError::NullPointer(_)));
    assert_eq!(0, s.mock_s3.count());
}

#[tokio::test]
async fn read_s3_failure_throws_s3_exception() {
    let s = setup().await;
    s.mock_s3.when_get_object(Err(AwsServiceError::service(
        AwsService::S3,
        None,
        Some("Failed to read from S3".into()),
    )));

    let exception = s
        .block_reader
        .read_with_schema(&s.spill_location, Some(&s.encryption_key), s.schema.clone())
        .await
        .unwrap_err();

    assert!(
        matches!(&exception, FederationError::AwsService(e) if e.is_service_exception_of(AwsService::S3)),
        "{exception:?}"
    );
    assert_eq!("Failed to read from S3", exception.to_string());
    assert_eq!(1, s.mock_s3.count());
}

#[tokio::test]
async fn read_corrupted_encrypted_data_throws_athena_connector_exception() {
    let s = setup().await;
    s.mock_s3.when_get_object(Ok(vec![1, 2, 3, 4, 5]));
    let ex = s
        .block_reader
        .read_with_schema(&s.spill_location, Some(&s.encryption_key), s.schema.clone())
        .await
        .unwrap_err();
    assert_connector_exception(&ex);
    assert_eq!(1, s.mock_s3.count());
}

#[tokio::test]
async fn read_mismatched_encryption_key_throws_athena_connector_exception() {
    let s = setup().await;
    let block_crypto = AesGcmBlockCrypto::new(s.allocator.clone());
    let spilled_bytes = block_crypto
        .encrypt(Some(&s.encryption_key), &s.expected)
        .unwrap();
    let wrong_key = LocalKeyFactory::new().create().await.unwrap();
    s.mock_s3.when_get_object(Ok(spilled_bytes));

    let ex = s
        .block_reader
        .read_with_schema(&s.spill_location, Some(&wrong_key), s.schema.clone())
        .await
        .unwrap_err();

    assert_connector_exception(&ex);
    assert_eq!(1, s.mock_s3.count());
}

#[tokio::test]
async fn read_null_schema_throws_null_pointer_exception() {
    let s = setup().await;
    let block_crypto = AesGcmBlockCrypto::new(s.allocator.clone());
    let spilled_bytes = block_crypto
        .encrypt(Some(&s.encryption_key), &s.expected)
        .unwrap();
    s.mock_s3.when_get_object(Ok(spilled_bytes));

    let ex = s
        .block_reader
        .read_with_schema(&s.spill_location, Some(&s.encryption_key), None)
        .await
        .unwrap_err();

    assert!(matches!(ex, FederationError::NullPointer(_)));
}

#[tokio::test]
async fn read_unencrypted_block_with_null_encryption_key_succeeds() {
    let s = setup().await;
    let block_crypto = NoOpBlockCrypto::new(s.allocator.clone());
    let spilled_bytes = block_crypto.encrypt(None, &s.expected).unwrap();
    s.mock_s3.when_get_object(Ok(spilled_bytes));

    let actual = s
        .block_reader
        .read_with_schema(&s.spill_location, None, s.schema.clone())
        .await
        .unwrap();

    assert_eq!(s.expected, actual);
    assert_eq!(1, s.mock_s3.count());
}
