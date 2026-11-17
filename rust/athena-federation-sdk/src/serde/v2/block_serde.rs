use std::sync::Arc;

use arrow::datatypes::Schema;

use crate::data::{Block, BlockAllocator};
use crate::exceptions::Result;
use crate::serde::arrow_ipc::{block_batch_bytes, record_batch_from_bytes, IpcFormat};
use crate::serde::base::{
    assert_field_name, deserialize_nullable, get_next_binary_field, get_next_string_field,
    serialize_nullable,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{AllocatorSource, SerDeContext};

const ALLOCATOR_ID_FIELD_NAME: &str = "aId";
const SCHEMA_FIELD_NAME: &str = "schema";
const BATCH_FIELD_NAME: &str = "records";

pub fn serialize(block: Option<&Block>, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    serialize_nullable(jgen, block, |b, j| do_serialize(b, j, version))
}

pub fn do_serialize(block: &Block, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    let format = crate::serde::ipc_format_for(version);
    jgen.write_string_field(ALLOCATOR_ID_FIELD_NAME, Some(block.allocator_id()))?;
    jgen.write_field_name(SCHEMA_FIELD_NAME)?;
    super::schema_serde::serialize_with_format(block.schema(), jgen, format)?;
    jgen.write_field_name(BATCH_FIELD_NAME)?;
    if block.row_count() > 0 {
        jgen.write_binary(&block_batch_bytes(block, format)?)
    } else {
        jgen.write_string(Some(""))
    }
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Option<Block>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

pub fn deserialize_schema_for_version(
    jparser: &mut JsonParser<'_>,
    version: i32,
) -> Result<Schema> {
    if version <= 2 {
        super::schema_serde::deserialize(jparser)
    } else {
        crate::serde::v3::schema_serde_v3::deserialize(jparser)
    }
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Block> {
    let allocator_id = get_next_string_field(jparser, ALLOCATOR_ID_FIELD_NAME)?;
    assert_field_name(jparser, Some(SCHEMA_FIELD_NAME))?;
    let schema = deserialize_schema_for_version(jparser, ctx.version)?;
    let batch_bytes = get_next_binary_field(jparser, BATCH_FIELD_NAME)?;
    with_allocator(ctx, allocator_id.as_deref(), |allocator| {
        let mut block = allocator.create_block(Arc::new(schema));
        if !batch_bytes.is_empty() {
            let batch = record_batch_from_bytes(allocator, &batch_bytes)?;
            block.load_arrow_record_batch(&batch)?;
        }
        Ok(block)
    })
}

pub fn with_allocator<T, F>(ctx: &SerDeContext<'_>, allocator_id: Option<&str>, f: F) -> Result<T>
where
    F: FnOnce(&dyn BlockAllocator) -> Result<T>,
{
    match ctx.allocators {
        AllocatorSource::Allocator(a) => f(a),
        AllocatorSource::Registry(r) => {
            let allocator = r.get_or_create_allocator(allocator_id.unwrap_or("null"));
            f(allocator.as_ref())
        }
    }
}

pub fn ipc_format(version: i32) -> IpcFormat {
    crate::serde::ipc_format_for(version)
}
