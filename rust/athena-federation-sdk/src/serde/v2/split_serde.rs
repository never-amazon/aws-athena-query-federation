use std::collections::HashMap;

use crate::domain::split::Split;
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{
    assert_field_name, deserialize_nullable, serialize_nullable, validate_object_start_token,
};
use crate::serde::java_hash::java_hash_map_order;
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

const SPILL_LOCATION_FIELD: &str = "spillLocation";
const ENCRYPTION_KEY_FIELD: &str = "encryptionKey";
const PROPERTIES_FIELD: &str = "properties";

pub fn serialize(split: Option<&Split>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, split, do_serialize)
}

pub fn do_serialize(split: &Split, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_field_name(SPILL_LOCATION_FIELD)?;
    super::spill_location_serde::serialize(split.spill_location(), jgen)?;
    jgen.write_field_name(ENCRYPTION_KEY_FIELD)?;
    match split.encryption_key() {
        Some(key) => super::encryption_key_serde::serialize(Some(key), jgen)?,
        None => jgen.write_null()?,
    }
    jgen.write_object_field_start(PROPERTIES_FIELD)?;
    let properties = split.properties();
    for key in java_hash_map_order(properties.keys()) {
        jgen.write_field_name(key)?;
        jgen.write_string(properties.get(key).and_then(|v| v.as_deref()))?;
    }
    jgen.write_end_object()
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<Split>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<Split> {
    assert_field_name(jparser, Some(SPILL_LOCATION_FIELD))?;
    let spill_location = super::spill_location_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(ENCRYPTION_KEY_FIELD))?;
    let encryption_key = super::encryption_key_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(PROPERTIES_FIELD))?;
    validate_object_start_token(jparser.next_token()?)?;
    let mut properties: HashMap<String, String> = HashMap::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndObject) => break,
            None => {
                return Err(FederationError::io(
                    "Unexpected end-of-input: expected close marker for OBJECT",
                ))
            }
            Some(_) => {
                let key = jparser.get_value_as_string();
                jparser.next_token()?;
                let value = jparser.get_value_as_string();
                let key = key.ok_or_else(|| {
                    FederationError::null_pointer(format!(
                        "null key in entry: null={}",
                        value.as_deref().unwrap_or("null")
                    ))
                })?;
                let value = value.ok_or_else(|| {
                    FederationError::null_pointer(format!("null value in entry: {key}=null"))
                })?;
                if properties.contains_key(&key) {
                    return Err(FederationError::IllegalArgument(format!(
                        "Multiple entries with same key: {key}"
                    )));
                }
                properties.insert(key, value);
            }
        }
    }
    Ok(Split::new(spill_location, encryption_key, properties))
}
