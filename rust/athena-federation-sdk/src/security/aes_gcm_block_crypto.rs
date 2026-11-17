use std::sync::Arc;

use aes_gcm::aead::consts::U12;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::aes::Aes192;
use aes_gcm::{Aes128Gcm, Aes256Gcm, AesGcm, Nonce};
use arrow::datatypes::SchemaRef;

use super::block_crypto::BlockCrypto;
use super::crypto_support::{deserialize_block, internal_error, invalid_input, serialize_block};
use super::encryption_key::EncryptionKey;
use crate::data::{Block, BlockAllocator};
use crate::exceptions::{FederationError, Result};

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

#[derive(Clone, Copy)]
enum Mode {
    Encrypt,
    Decrypt,
}

struct GcmCipher {
    cipher: Cipher,
    nonce: Nonce<U12>,
    mode: Mode,
}

impl GcmCipher {
    fn do_final(&self, input: &[u8]) -> Result<Vec<u8>> {
        if let Mode::Decrypt = self.mode {
            if input.len() < TAG_BYTES {
                return Err(internal_error("data too short"));
            }
        }
        let result = match (&self.cipher, self.mode) {
            (Cipher::Aes128(c), Mode::Encrypt) => c.encrypt(&self.nonce, input),
            (Cipher::Aes192(c), Mode::Encrypt) => c.encrypt(&self.nonce, input),
            (Cipher::Aes256(c), Mode::Encrypt) => c.encrypt(&self.nonce, input),
            (Cipher::Aes128(c), Mode::Decrypt) => c.decrypt(&self.nonce, input),
            (Cipher::Aes192(c), Mode::Decrypt) => c.decrypt(&self.nonce, input),
            (Cipher::Aes256(c), Mode::Decrypt) => c.decrypt(&self.nonce, input),
        };
        result.map_err(|_| match self.mode {
            Mode::Encrypt => internal_error("GCM encryption failed"),
            Mode::Decrypt => internal_error("mac check in GCM failed"),
        })
    }
}

pub struct AesGcmBlockCrypto {
    allocator: Arc<dyn BlockAllocator>,
}

impl AesGcmBlockCrypto {
    pub fn new(allocator: Arc<dyn BlockAllocator>) -> AesGcmBlockCrypto {
        AesGcmBlockCrypto { allocator }
    }

    fn make_cipher(mode: Mode, key: Option<&EncryptionKey>) -> Result<GcmCipher> {
        let key = key.ok_or_else(|| FederationError::NullPointer("key was null".into()))?;
        if key.get_nonce().len() != NONCE_BYTES {
            return Err(invalid_input(format!(
                "Expected {} nonce bytes but found {}",
                NONCE_BYTES,
                key.get_nonce().len()
            )));
        }
        tracing::debug!("Cipher key bytes length: {}", key.get_key().len());
        if key.get_key().is_empty() {
            return Err(invalid_input("Invalid key"));
        }
        let bad_key = |_| internal_error("Key length not 128/192/256 bits.");
        let cipher = match key.get_key().len() {
            16 => Cipher::Aes128(Box::new(
                Aes128Gcm::new_from_slice(key.get_key()).map_err(bad_key)?,
            )),
            24 => Cipher::Aes192(Box::new(
                Aes192Gcm::new_from_slice(key.get_key()).map_err(bad_key)?,
            )),
            32 => Cipher::Aes256(Box::new(
                Aes256Gcm::new_from_slice(key.get_key()).map_err(bad_key)?,
            )),
            _ => return Err(internal_error("Key length not 128/192/256 bits.")),
        };
        let nonce = Nonce::<U12>::try_from(key.get_nonce())
            .map_err(|_| invalid_input(format!("Expected {NONCE_BYTES} nonce bytes")))?;
        Ok(GcmCipher {
            cipher,
            nonce,
            mode,
        })
    }
}

impl BlockCrypto for AesGcmBlockCrypto {
    fn encrypt(&self, key: Option<&EncryptionKey>, block: &Block) -> Result<Vec<u8>> {
        let clear = serialize_block(block)?;
        Self::make_cipher(Mode::Encrypt, key)?.do_final(&clear)
    }

    fn encrypt_with_bytes(&self, key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>> {
        Self::make_cipher(Mode::Encrypt, key)?.do_final(bytes)
    }

    fn decrypt_with_schema(
        &self,
        key: Option<&EncryptionKey>,
        bytes: &[u8],
        schema: SchemaRef,
    ) -> Result<Block> {
        let clear = Self::make_cipher(Mode::Decrypt, key)?.do_final(bytes)?;
        deserialize_block(self.allocator.as_ref(), &clear, schema)
    }

    fn decrypt(&self, key: Option<&EncryptionKey>, bytes: &[u8]) -> Result<Vec<u8>> {
        Self::make_cipher(Mode::Decrypt, key)?.do_final(bytes)
    }
}
