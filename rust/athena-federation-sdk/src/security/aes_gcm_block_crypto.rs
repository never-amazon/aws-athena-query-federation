use aes_gcm::aead::consts::U12;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::aes::Aes192;
use aes_gcm::{Aes128Gcm, Aes256Gcm, AesGcm, Nonce};

use super::encryption_key::EncryptionKey;
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
