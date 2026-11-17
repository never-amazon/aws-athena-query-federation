use std::sync::Arc;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::{Block, BlockAllocator, BlockAllocatorImpl, BlockUtils, SchemaBuilder, Value};
use athena_federation_sdk::security::{
    AesGcmBlockCrypto, BlockCrypto, EncryptionKey, EncryptionKeyFactory, LocalKeyFactory,
};

struct Setup {
    allocator: Arc<BlockAllocatorImpl>,
    crypto: AesGcmBlockCrypto,
    key: EncryptionKey,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.allocator.close();
    }
}

async fn setup() -> Setup {
    let key_factory = LocalKeyFactory::new();
    Setup {
        allocator: Arc::new(BlockAllocatorImpl::new()),
        crypto: AesGcmBlockCrypto::new(Arc::new(BlockAllocatorImpl::new())),
        key: key_factory.create().await.unwrap(),
    }
}

fn set(block: &mut Block, field: &str, row: usize, value: impl Into<Value>) {
    BlockUtils::set_value(block.get_field_vector_mut(field).unwrap(), row, &value.into()).unwrap();
}

#[tokio::test]
async fn test() {
    let s = setup().await;
    let schema = Arc::new(
        SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", DataType::Utf8)
            .build(),
    );
    let mut expected = s.allocator.create_block(schema.clone());
    set(&mut expected, "col1", 1, 100);
    set(&mut expected, "col2", 1, "VarChar");
    set(&mut expected, "col1", 1, 101);
    set(&mut expected, "col2", 1, "VarChar1");
    expected.set_row_count(2);

    let cypher = s.crypto.encrypt(Some(&s.key), &expected).unwrap();
    let actual = s.crypto.decrypt_with_schema(Some(&s.key), &cypher, schema).unwrap();
    assert_eq!(expected, actual);
}

#[tokio::test]
async fn encrypt_raw_bytes_round_trip() {
    let s = setup().await;
    let plain = "hello spill".as_bytes();
    let encrypted = s.crypto.encrypt_with_bytes(Some(&s.key), plain).unwrap();
    assert_eq!(plain.len() + 16, encrypted.len());
    assert_eq!(plain, s.crypto.decrypt(Some(&s.key), &encrypted).unwrap().as_slice());
}

#[tokio::test]
async fn decrypt_with_invalid_bytes() {
    let s = setup().await;
    let key = LocalKeyFactory::new().create().await.unwrap();
    let invalid_bytes = [0u8; 16];
    let ex = s
        .crypto
        .decrypt(Some(&key), &invalid_bytes)
        .expect_err("Expected AthenaConnectorException was not thrown");
    assert!(ex.as_connector_exception().is_some());
    assert!(ex.to_string().to_lowercase().contains("mac check in gcm failed"));
}

#[tokio::test]
async fn make_cipher_with_invalid_nonce() {
    let s = setup().await;
    let invalid_key = EncryptionKey::new(vec![0u8; 32], vec![0u8; 5]);
    let ex = s
        .crypto
        .decrypt(Some(&invalid_key), &[0u8; 16])
        .expect_err("Expected AthenaConnectorException due to invalid nonce");
    assert!(ex.as_connector_exception().is_some());
    assert!(ex.to_string().contains("Expected 12 nonce bytes"));
}

#[tokio::test]
async fn make_cipher_with_empty_key() {
    let s = setup().await;
    let invalid_key = EncryptionKey::new(Vec::new(), vec![0u8; 12]);
    let ex = s
        .crypto
        .decrypt(Some(&invalid_key), &[0u8; 16])
        .expect_err("Expected AthenaConnectorException due to empty key");
    assert!(ex.as_connector_exception().is_some());
    assert!(ex.to_string().contains("Invalid key"));
}
