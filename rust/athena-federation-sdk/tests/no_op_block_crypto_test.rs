use athena_federation_sdk::data::block_utils;
use std::sync::Arc;

use arrow::array::Int32Array;
use arrow::datatypes::{DataType, Field, Schema};
use athena_federation_sdk::data::{BlockAllocator, BlockAllocatorImpl, Value};
use athena_federation_sdk::security::{BlockCrypto, EncryptionKey, KeySource};

struct Setup {
    allocator: Arc<BlockAllocatorImpl>,
    no_op_block_crypto: BlockCrypto,
    test_schema: Arc<Schema>,
    encryption_key: EncryptionKey,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.allocator.close();
    }
}

async fn setup() -> Setup {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    Setup {
        no_op_block_crypto: BlockCrypto::for_key(None),
        allocator,
        test_schema: Arc::new(Schema::empty()),
        encryption_key: KeySource::Local.create(None).await.unwrap(),
    }
}

fn int_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![Field::new("id", DataType::Int32, true)]))
}

#[tokio::test]
async fn encrypt_with_null_key() {
    let s = setup().await;
    let block = s.allocator.create_block(s.test_schema.clone());
    let encrypted = s.no_op_block_crypto.encrypt(&block).unwrap();
    assert!(!encrypted.is_empty());
}

#[tokio::test]
async fn encrypt_with_non_null_key_throws_exception() {
    let s = setup().await;
    let block = s.allocator.create_block(s.test_schema.clone());
    let crypto = BlockCrypto::for_key(Some(s.encryption_key.clone()));
    assert_eq!(BlockCrypto::AesGcm(s.encryption_key.clone()), crypto);
    assert_ne!(
        s.no_op_block_crypto.encrypt(&block).unwrap(),
        crypto.encrypt(&block).unwrap()
    );
}

#[tokio::test]
async fn encrypt_bytes_passthrough() {
    let s = setup().await;
    let input_bytes = [1u8, 2, 3, 4, 5];
    assert_eq!(
        input_bytes.as_slice(),
        s.no_op_block_crypto
            .encrypt_with_bytes(&input_bytes)
            .unwrap()
            .as_slice()
    );
}

#[tokio::test]
async fn encrypt_bytes_with_non_null_key_throws_exception() {
    let s = setup().await;
    let crypto = BlockCrypto::for_key(Some(s.encryption_key.clone()));
    assert_eq!(BlockCrypto::AesGcm(s.encryption_key.clone()), crypto);
    assert_ne!(vec![1u8], crypto.encrypt_with_bytes(&[1u8]).unwrap());
}

#[tokio::test]
async fn decrypt_bytes_passthrough() {
    let s = setup().await;
    let input_bytes = [1u8, 2, 3, 4, 5];
    let result = s.no_op_block_crypto.decrypt(&input_bytes).unwrap();
    assert_eq!(input_bytes.as_slice(), result.as_slice());
}

#[tokio::test]
async fn decrypt_with_schema() {
    let s = setup().await;
    let schema = int_schema();
    let mut block = s.allocator.create_block(schema.clone());
    block_utils::set_value(
        block.get_field_vector_mut("id").unwrap(),
        0,
        &Value::Int(42),
    )
    .unwrap();
    block.set_row_count(1);

    let encrypted = s.no_op_block_crypto.encrypt(&block).unwrap();
    let decrypted_block = s
        .no_op_block_crypto
        .decrypt_with_schema(s.allocator.as_ref(), &encrypted, schema.clone())
        .unwrap();

    assert_eq!(&schema, decrypted_block.schema());
    let batch = decrypted_block.get_record_batch().unwrap();
    let result_vector = batch
        .column(0)
        .as_any()
        .downcast_ref::<Int32Array>()
        .unwrap();
    assert_eq!(1, result_vector.len());
    assert_eq!(42, result_vector.value(0));
}

#[tokio::test]
async fn decrypt_with_corrupted_bytes_throws_exception() {
    let s = setup().await;
    let corrupted_bytes = [0x01u8, 0x02, 0x03, 0x04];
    let ex = s
        .no_op_block_crypto
        .decrypt_with_schema(s.allocator.as_ref(), &corrupted_bytes, int_schema())
        .unwrap_err();
    assert!(ex.as_connector_exception().is_some());
}
