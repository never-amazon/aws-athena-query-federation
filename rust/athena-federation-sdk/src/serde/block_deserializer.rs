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
    FederationError::null_pointer(format!(
        "Cannot invoke \"com.fasterxml.jackson.databind.JsonNode\" method because field \"{field}\" is null"
    ))
}

pub fn deserialize_tree(node: &serde_json::Value, ctx: &SerDeContext<'_>) -> Result<Block> {
    node.get(ALLOCATOR_ID_FIELD_NAME)
        .ok_or_else(|| missing(ALLOCATOR_ID_FIELD_NAME))?;
    let schema_bytes = binary_value(
        node.get(SCHEMA_FIELD_NAME)
            .ok_or_else(|| missing(SCHEMA_FIELD_NAME))?,
    )?;
    let batch_bytes = binary_value(
        node.get(BATCH_FIELD_NAME)
            .ok_or_else(|| missing(BATCH_FIELD_NAME))?,
    )?;
    let schema = schema_from_bytes(&schema_bytes)?;
    let mut block = ctx.allocator.create_block(Arc::new(schema));
    if !batch_bytes.is_empty() {
        let batch = record_batch_from_bytes(ctx.allocator, &batch_bytes)?;
        block.load_arrow_record_batch(&batch)?;
    }
    Ok(block)
}
