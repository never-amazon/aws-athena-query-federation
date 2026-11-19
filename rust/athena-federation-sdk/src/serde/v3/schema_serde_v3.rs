use arrow::datatypes::Schema;

use crate::exceptions::{FederationError, Result};
use crate::serde::arrow_ipc::{schema_from_bytes, IpcFormat};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::SerdeVersion;

pub fn serialize(schema: &Schema, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::schema_serde::serialize_with_format(schema, jgen, IpcFormat::ArrowDefault)
}

pub fn serialize_for_version(
    schema: &Schema,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    if version <= SerdeVersion::V2 {
        crate::serde::v2::schema_serde::serialize(schema, jgen)
    } else {
        serialize(schema, jgen)
    }
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Schema> {
    if jparser.next_token()? != Some(JsonToken::ValueString) {
        return Err(FederationError::invalid_input(format!(
            "Expected {} found {}",
            JsonToken::ValueString,
            jparser.text_or_null()
        )));
    }
    let schema_bytes = jparser.get_binary_value()?;
    schema_from_bytes(&schema_bytes)
}
