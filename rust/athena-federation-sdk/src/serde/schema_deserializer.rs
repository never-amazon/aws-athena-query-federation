use arrow::datatypes::Schema;

use crate::exceptions::{FederationError, Result};
use crate::serde::arrow_ipc::schema_from_bytes;
use crate::serde::json::decode_base64;
use crate::serde::schema_serializer::SCHEMA_FIELD_NAME;

pub fn deserialize_tree(node: &serde_json::Value) -> Result<Schema> {
    let field = node.get(SCHEMA_FIELD_NAME).ok_or_else(|| {
        FederationError::null_pointer("Cannot invoke \"com.fasterxml.jackson.databind.JsonNode.binaryValue()\" because the return value of \"com.fasterxml.jackson.databind.JsonNode.get(String)\" is null")
    })?;
    let bytes = binary_value(field)?;
    schema_from_bytes(&bytes)
}

pub fn binary_value(node: &serde_json::Value) -> Result<Vec<u8>> {
    match node {
        serde_json::Value::String(s) => decode_base64(s),
        _ => Ok(Vec::new()),
    }
}
