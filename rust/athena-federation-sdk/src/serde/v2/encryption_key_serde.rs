use crate::exceptions::Result;
use crate::security::EncryptionKey;
use crate::serde::base::{deserialize_nullable, get_next_binary_field, serialize_nullable};
use crate::serde::json::{JsonGenerator, JsonParser};

const KEY_FIELD: &str = "key";
const NONCE_FIELD: &str = "nonce";

pub fn serialize(encryption_key: Option<&EncryptionKey>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, encryption_key, do_serialize)
}

pub fn do_serialize(encryption_key: &EncryptionKey, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_binary_field(KEY_FIELD, encryption_key.key())?;
    jgen.write_binary_field(NONCE_FIELD, encryption_key.nonce())
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<EncryptionKey>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<EncryptionKey> {
    let key = get_next_binary_field(jparser, KEY_FIELD)?;
    let nonce = get_next_binary_field(jparser, NONCE_FIELD)?;
    Ok(EncryptionKey::new(key, nonce))
}
