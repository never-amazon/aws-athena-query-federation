use arrow::datatypes::Schema;

use crate::exceptions::Result;
use crate::serde::arrow_ipc::{schema_to_bytes, IpcFormat};
use crate::serde::json::JsonGenerator;

pub const SCHEMA_FIELD_NAME: &str = "schema";

pub fn serialize(schema: &Schema, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_start_object()?;
    let bytes = schema_to_bytes(schema, IpcFormat::AthenaFederationLegacy)?;
    jgen.write_binary_field(SCHEMA_FIELD_NAME, &bytes)?;
    jgen.write_end_object()
}
