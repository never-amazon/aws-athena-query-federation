use crate::data::Block;
use crate::exceptions::Result;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SERDE_VERSION_THREE};

pub fn serialize(block: Option<&Block>, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::block_serde::serialize(block, jgen, SERDE_VERSION_THREE)
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Option<Block>> {
    let ctx = SerDeContext {
        version: ctx.version.max(SERDE_VERSION_THREE),
        allocator: ctx.allocator,
    };
    crate::serde::v2::block_serde::deserialize(jparser, &ctx)
}
