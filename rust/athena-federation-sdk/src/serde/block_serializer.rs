use crate::data::Block;
use crate::exceptions::Result;
use crate::serde::arrow_ipc::{block_batch_bytes, schema_to_bytes, IpcFormat};
use crate::serde::json::JsonGenerator;

pub const ALLOCATOR_ID_FIELD_NAME: &str = "aId";
pub const SCHEMA_FIELD_NAME: &str = "schema";
pub const BATCH_FIELD_NAME: &str = "records";

pub fn serialize(block: &Block, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    jgen.write_string_field(ALLOCATOR_ID_FIELD_NAME, Some(block.allocator_id()))?;
    let schema_bytes = schema_to_bytes(block.schema(), IpcFormat::AthenaFederationLegacy)?;
    jgen.write_binary_field(SCHEMA_FIELD_NAME, &schema_bytes)?;
    let batch_bytes = if block.row_count() > 0 {
        block_batch_bytes(block, IpcFormat::AthenaFederationLegacy)?
    } else {
        Vec::new()
    };
    jgen.write_binary_field(BATCH_FIELD_NAME, &batch_bytes)?;
    jgen.write_end_object()
}
