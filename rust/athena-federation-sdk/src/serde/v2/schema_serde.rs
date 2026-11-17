use arrow::datatypes::Schema;

use crate::exceptions::{FederationError, Result};
use crate::serde::arrow_ipc::{schema_from_bytes, schema_to_bytes, IpcFormat};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

pub fn serialize(schema: &Schema, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_with_format(schema, jgen, IpcFormat::AthenaFederationLegacy)
}

pub fn serialize_with_format(schema: &Schema, jgen: &mut JsonGenerator, format: IpcFormat) -> Result<()> {
    let bytes = schema_to_bytes(schema, format)?;
    jgen.write_binary(&bytes)
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Schema> {
    if jparser.current_token() == Some(JsonToken::ValueString) || jparser.next_token()? == Some(JsonToken::ValueString) {
        let schema_bytes = jparser.get_binary_value()?;
        schema_from_bytes(&schema_bytes)
    } else {
        Err(FederationError::invalid_input(format!(
            "Expected {} found {}",
            JsonToken::ValueString,
            jparser.text_or_null()
        )))
    }
}
