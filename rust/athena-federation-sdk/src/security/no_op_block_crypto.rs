use std::sync::Arc;

use arrow::datatypes::SchemaRef;

use super::block_crypto::BlockCrypto;
use super::crypto_support::{deserialize_block, invalid_input, serialize_block};
use super::encryption_key::EncryptionKey;
use crate::data::{Block, BlockAllocator};
use crate::exceptions::Result;

const REAL_KEY_MESSAGE: &str =
    "Real key provided to NoOpBlockCrypto, likely indicates you wanted real crypto.";

pub struct NoOpBlockCrypto {
    allocator: Arc<dyn BlockAllocator>,
}

impl NoOpBlockCrypto {
    pub fn new(allocator: Arc<dyn BlockAllocator>) -> NoOpBlockCrypto {
        NoOpBlockCrypto { allocator }
    }
}

impl BlockCrypto for NoOpBlockCrypto {
    fn encrypt(&self, key: Option<&EncryptionKey>, block: &Block) -> Result<Vec<u8>> {
        if key.is_some() {
            return Err(invalid_input(REAL_KEY_MESSAGE));
        }
        serialize_block(block)
    }

    fn encrypt_with_bytes(&self, key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>> {
        if key.is_some() {
            return Err(invalid_input(REAL_KEY_MESSAGE));
        }
        Ok(bytes.to_vec())
    }

    fn decrypt_with_schema(
        &self,
        _key: Option<&EncryptionKey>,
        bytes: &[u8],
        schema: SchemaRef,
    ) -> Result<Block> {
        deserialize_block(self.allocator.as_ref(), bytes, schema)
    }

    fn decrypt(&self, _key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>> {
        Ok(bytes.to_vec())
    }
}
