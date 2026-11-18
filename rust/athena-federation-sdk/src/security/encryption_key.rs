use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EncryptionKey {
    key: Vec<u8>,
    nonce: Vec<u8>,
}

impl EncryptionKey {
    pub fn new(key: Vec<u8>, nonce: Vec<u8>) -> EncryptionKey {
        EncryptionKey { key, nonce }
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }
}

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncryptionKey{<hidden>}")
    }
}
