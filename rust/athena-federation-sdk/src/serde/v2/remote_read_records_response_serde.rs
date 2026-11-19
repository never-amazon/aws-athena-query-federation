use crate::exceptions::{FederationError, Result};
use crate::records::RemoteReadRecordsResponse;
use crate::serde::base::{
    assert_field_name, get_next_string_field, require_non_null, validate_array_start,
    validate_object_end, validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::{SerDeContext, SerdeVersion};

const CATALOG_NAME_FIELD: &str = "catalogName";
const SCHEMA_FIELD: &str = "schema";
const REMOTE_BLOCKS_FIELD: &str = "remoteBlocks";
const ENCRYPTION_KEY_FIELD: &str = "encryptionKey";

pub fn do_typed_serialize(
    response: &RemoteReadRecordsResponse,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.catalog_name()))?;
    jgen.write_field_name(SCHEMA_FIELD)?;
    crate::serde::v3::schema_serde_v3::serialize_for_version(response.schema(), jgen, version)?;
    jgen.write_array_field_start(REMOTE_BLOCKS_FIELD)?;
    for spill_location in response.remote_blocks() {
        super::spill_location_serde::serialize(Some(spill_location), jgen)?;
    }
    jgen.write_end_array()?;
    jgen.write_field_name(ENCRYPTION_KEY_FIELD)?;
    super::encryption_key_serde::serialize(response.encryption_key(), jgen)
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<RemoteReadRecordsResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(SCHEMA_FIELD))?;
    let schema = super::block_serde::deserialize_schema_for_version(jparser, ctx.version)?;
    assert_field_name(jparser, Some(REMOTE_BLOCKS_FIELD))?;
    validate_array_start(jparser)?;
    let mut remote_blocks = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::io(
                    "Unexpected end-of-input: expected close marker for ARRAY",
                ))
            }
            token => {
                validate_object_start_token(token)?;
                remote_blocks.push(super::spill_location_serde::do_deserialize(jparser)?);
                validate_object_end(jparser)?;
            }
        }
    }
    assert_field_name(jparser, Some(ENCRYPTION_KEY_FIELD))?;
    let encryption_key = super::encryption_key_serde::deserialize(jparser)?;
    Ok(RemoteReadRecordsResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        schema,
        remote_blocks,
        encryption_key,
    ))
}
