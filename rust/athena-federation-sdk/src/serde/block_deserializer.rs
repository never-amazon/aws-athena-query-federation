use std::sync::Arc;

use crate::data::Block;
use crate::exceptions::{FederationError, Result};
use crate::serde::arrow_ipc::{record_batch_from_bytes, schema_from_bytes};
use crate::serde::block_serializer::{
    ALLOCATOR_ID_FIELD_NAME, BATCH_FIELD_NAME, SCHEMA_FIELD_NAME,
};
use crate::serde::schema_deserializer::binary_value;
use crate::serde::SerDeContext;

fn missing(field: &str) -> FederationError {
    FederationError::NullPointer(format!(
        "Cannot invoke \"com.fasterxml.jackson.databind.JsonNode\" method because field \"{field}\" is null"
    ))
}

pub fn deserialize_tree(node: &serde_json::Value, ctx: &SerDeContext<'_>) -> Result<Block> {
    let allocator_id = node
        .get(ALLOCATOR_ID_FIELD_NAME)
        .ok_or_else(|| missing(ALLOCATOR_ID_FIELD_NAME))?;
    let allocator_id = match allocator_id {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    };
    let schema_bytes = binary_value(
        node.get(SCHEMA_FIELD_NAME)
            .ok_or_else(|| missing(SCHEMA_FIELD_NAME))?,
    )?;
    let batch_bytes = binary_value(
        node.get(BATCH_FIELD_NAME)
            .ok_or_else(|| missing(BATCH_FIELD_NAME))?,
    )?;
    let schema = schema_from_bytes(&schema_bytes)?;
    crate::serde::v2::block_serde::with_allocator(ctx, Some(&allocator_id), |allocator| {
        let mut block = allocator.create_block(Arc::new(schema));
        if !batch_bytes.is_empty() {
            let batch = record_batch_from_bytes(allocator, &batch_bytes)?;
            block.load_arrow_record_batch(&batch)?;
        }
        Ok(block)
    })
}
