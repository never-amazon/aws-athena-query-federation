use crate::data::ArrowRecordBatch;
use crate::exceptions::{FederationError, Result};
use crate::serde::arrow_ipc::record_batch_from_bytes;
use crate::serde::base::serialize_object;
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::SerDeContext;

pub fn serialize(batch: &ArrowRecordBatch, jgen: &mut JsonGenerator) -> Result<()> {
    let mut out = Vec::new();
    batch.serialize(&mut out)?;
    serialize_object(jgen, |j| j.write_binary(&out))
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<ArrowRecordBatch> {
    if jparser.next_token()? != Some(JsonToken::ValueEmbeddedObject) {
        return Err(FederationError::invalid_input(format!(
            "Expecting {} but found {}",
            JsonToken::ValueString,
            jparser.text_or_null()
        )));
    }
    let bytes = jparser.get_binary_value()?;
    crate::serde::v2::block_serde::with_allocator(ctx, None, |a| record_batch_from_bytes(a, &bytes))
}
