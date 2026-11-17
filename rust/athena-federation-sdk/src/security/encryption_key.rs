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

    pub fn get_key(&self) -> &[u8] {
        &self.key
    }

    pub fn get_nonce(&self) -> &[u8] {
        &self.nonce
    }

    pub fn hash_code(&self) -> i32 {
        fn arrays_hash(b: &[u8]) -> i32 {
            b.iter().fold(1i32, |h, x| {
                h.wrapping_mul(31).wrapping_add(i32::from(x.cast_signed()))
            })
        }
        arrays_hash(&self.key)
            .wrapping_add(31)
            .wrapping_add(arrays_hash(&self.nonce))
    }
}

impl fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EncryptionKey{<hidden>}")
    }
}
