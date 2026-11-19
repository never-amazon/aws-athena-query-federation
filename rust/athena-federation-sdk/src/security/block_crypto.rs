use aes_gcm::aead::consts::U12;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::aes::Aes192;
use aes_gcm::{Aes128Gcm, Aes256Gcm, AesGcm, Nonce};
use arrow::datatypes::SchemaRef;

use crate::data::{Block, BlockAllocator, RecordBatchSerDe};
use crate::exceptions::{
    AthenaConnectorException, FederationError, FederationSourceErrorCode, Result,
};
use crate::security::encryption_key::EncryptionKey;

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

pub const GCM_TAG_LENGTH_BITS: usize = 16 * 8;
pub const NONCE_BYTES: usize = 12;
pub const KEY_BYTES: usize = 32;
pub const KEYSPEC: &str = "AES";
pub const ALGO: &str = "AES/GCM/NoPadding";

const TAG_BYTES: usize = GCM_TAG_LENGTH_BITS / 8;

type Aes192Gcm = AesGcm<Aes192, U12>;

enum Cipher {
    Aes128(Box<Aes128Gcm>),
    Aes192(Box<Aes192Gcm>),
    Aes256(Box<Aes256Gcm>),
}

impl Cipher {
    fn encrypt(&self, nonce: &Nonce<U12>, input: &[u8]) -> aes_gcm::aead::Result<Vec<u8>> {
        match self {
            Cipher::Aes128(c) => c.encrypt(nonce, input),
            Cipher::Aes192(c) => c.encrypt(nonce, input),
            Cipher::Aes256(c) => c.encrypt(nonce, input),
        }
    }

    fn decrypt(&self, nonce: &Nonce<U12>, input: &[u8]) -> aes_gcm::aead::Result<Vec<u8>> {
        match self {
            Cipher::Aes128(c) => c.decrypt(nonce, input),
            Cipher::Aes192(c) => c.decrypt(nonce, input),
            Cipher::Aes256(c) => c.decrypt(nonce, input),
        }
    }
}

fn make_cipher(key: &EncryptionKey) -> Result<(Cipher, Nonce<U12>)> {
    if key.nonce().len() != NONCE_BYTES {
        return Err(FederationError::invalid_input(format!(
            "Expected {} nonce bytes but found {}",
            NONCE_BYTES,
            key.nonce().len()
        )));
    }
    tracing::debug!("Cipher key bytes length: {}", key.key().len());
    if key.key().is_empty() {
        return Err(FederationError::invalid_input("Invalid key"));
    }
    let bad_key = |_| FederationError::internal("Key length not 128/192/256 bits.");
    let cipher = match key.key().len() {
        16 => Cipher::Aes128(Box::new(
            Aes128Gcm::new_from_slice(key.key()).map_err(bad_key)?,
        )),
        24 => Cipher::Aes192(Box::new(
            Aes192Gcm::new_from_slice(key.key()).map_err(bad_key)?,
        )),
        32 => Cipher::Aes256(Box::new(
            Aes256Gcm::new_from_slice(key.key()).map_err(bad_key)?,
        )),
        _ => {
            return Err(FederationError::internal(
                "Key length not 128/192/256 bits.",
            ))
        }
    };
    let nonce = Nonce::<U12>::try_from(key.nonce()).map_err(|_| {
        FederationError::invalid_input(format!("Expected {NONCE_BYTES} nonce bytes"))
    })?;
    Ok((cipher, nonce))
}

pub(crate) fn gcm_encrypt(key: &EncryptionKey, input: &[u8]) -> Result<Vec<u8>> {
    let (cipher, nonce) = make_cipher(key)?;
    cipher
        .encrypt(&nonce, input)
        .map_err(|_| FederationError::internal("GCM encryption failed"))
}

pub(crate) fn gcm_decrypt(key: &EncryptionKey, input: &[u8]) -> Result<Vec<u8>> {
    let (cipher, nonce) = make_cipher(key)?;
    if input.len() < TAG_BYTES {
        return Err(FederationError::internal("data too short"));
    }
    cipher
        .decrypt(&nonce, input)
        .map_err(|_| FederationError::internal("mac check in GCM failed"))
}

fn connector_with_response(response: FederationError) -> FederationError {
    let message = response.to_string();
    FederationError::AthenaConnector(
        AthenaConnectorException::new(FederationSourceErrorCode::InternalServiceException, message)
            .with_response(response),
    )
}

fn wrap_io(err: FederationError) -> FederationError {
    match err {
        FederationError::Io { .. } => connector_with_response(err),
        FederationError::Arrow(e) => {
            connector_with_response(FederationError::from(std::io::Error::other(e)))
        }
        other => other,
    }
}

pub(crate) fn serialize_block(block: &Block) -> Result<Vec<u8>> {
    let batch = block.get_arrow_record_batch().map_err(wrap_io)?;
    let mut out = Vec::new();
    batch.serialize(&mut out).map_err(wrap_io)?;
    Ok(out)
}

pub(crate) fn deserialize_block(
    allocator: &dyn BlockAllocator,
    bytes: &[u8],
    schema: SchemaRef,
) -> Result<Block> {
    let mut result_block = allocator.create_block(schema);
    let batch = RecordBatchSerDe::new(allocator)
        .deserialize(bytes)
        .map_err(wrap_io)?;
    result_block
        .load_arrow_record_batch(&batch)
        .map_err(wrap_io)?;
    Ok(result_block)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exceptions::JavaThrowable;
    use std::error::Error as _;

    #[test]
    fn arrow_error_becomes_connector_exception_with_io_exception_response() {
        let err = wrap_io(FederationError::from(arrow::error::ArrowError::IpcError(
            "bad ipc".into(),
        )));
        let exception = err.as_connector_exception().unwrap();
        assert_eq!(
            Some(FederationSourceErrorCode::InternalServiceException),
            Some(exception.code)
        );
        assert_eq!("Ipc error: bad ipc", exception.message);
        assert!(exception.source().is_none());
        let response = exception.response.as_deref().unwrap();
        assert_eq!("java.io.IOException", response.java_class_name());
    }
}
