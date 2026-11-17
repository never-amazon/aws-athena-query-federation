use arrow::datatypes::SchemaRef;

use super::encryption_key::EncryptionKey;
use crate::data::Block;
use crate::exceptions::{FederationError, Result};

pub trait BlockCrypto: Send + Sync {
    fn encrypt(&self, key: Option<&EncryptionKey>, block: &Block) -> Result<Vec<u8>>;

    fn encrypt_with_bytes(&self, key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>>;

    fn decrypt_with_schema(
        &self,
        key: Option<&EncryptionKey>,
        bytes: &[u8],
        schema: SchemaRef,
    ) -> Result<Block>;

    fn decrypt(&self, key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>>;
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
