use async_trait::async_trait;

use super::aes_gcm_block_crypto::{KEY_BYTES, NONCE_BYTES};
use super::encryption_key::EncryptionKey;
use super::encryption_key_factory::EncryptionKeyFactory;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalKeyFactory;

impl LocalKeyFactory {
    pub fn new() -> LocalKeyFactory {
        LocalKeyFactory
    }

    pub fn create_key(&self) -> Result<EncryptionKey> {
        let mut key = vec![0u8; KEY_BYTES];
        let mut nonce = vec![0u8; NONCE_BYTES];
        getrandom::fill(&mut key)
            .and_then(|_| getrandom::fill(&mut nonce))
            .map_err(|e| FederationError::internal(e.to_string()))?;
        Ok(EncryptionKey::new(key, nonce))
    }
}

#[async_trait]
impl EncryptionKeyFactory for LocalKeyFactory {
    async fn create(&self) -> Result<EncryptionKey> {
        self.create_key()
    }
}
