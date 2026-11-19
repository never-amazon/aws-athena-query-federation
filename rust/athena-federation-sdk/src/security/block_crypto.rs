use arrow::datatypes::SchemaRef;

use super::aes_gcm_block_crypto::{gcm_decrypt, gcm_encrypt};
use super::crypto_support::{deserialize_block, serialize_block};
use super::encryption_key::EncryptionKey;
use crate::data::{Block, BlockAllocator};
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockCrypto {
    NoOp,
    AesGcm(EncryptionKey),
}

impl BlockCrypto {
    pub fn for_key(key: Option<EncryptionKey>) -> BlockCrypto {
        key.map_or(BlockCrypto::NoOp, BlockCrypto::AesGcm)
    }

    pub fn encrypt(&self, block: &Block) -> Result<Vec<u8>> {
        let clear = serialize_block(block)?;
        match self {
            BlockCrypto::NoOp => Ok(clear),
            BlockCrypto::AesGcm(key) => gcm_encrypt(key, &clear),
        }
    }

    pub fn encrypt_with_bytes(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        match self {
            BlockCrypto::NoOp => Ok(bytes.to_vec()),
            BlockCrypto::AesGcm(key) => gcm_encrypt(key, bytes),
        }
    }

    pub fn decrypt_with_schema(
        &self,
        allocator: &dyn BlockAllocator,
        bytes: &[u8],
        schema: SchemaRef,
    ) -> Result<Block> {
        match self {
            BlockCrypto::NoOp => deserialize_block(allocator, bytes, schema),
            BlockCrypto::AesGcm(key) => {
                deserialize_block(allocator, &gcm_decrypt(key, bytes)?, schema)
            }
        }
    }

    pub fn decrypt(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        match self {
            BlockCrypto::NoOp => Ok(bytes.to_vec()),
            BlockCrypto::AesGcm(key) => gcm_decrypt(key, bytes),
        }
    }
}

pub(crate) async fn run_blocking<T, F>(work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| FederationError::internal(e.to_string()))?
}
