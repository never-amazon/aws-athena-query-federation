use crate::exceptions::Result;
use crate::records::ReadRecordsResponse;
use crate::serde::base::{assert_field_name, get_next_string_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const CATALOG_NAME_FIELD: &str = "catalogName";
const RECORDS_FIELD: &str = "records";

pub fn do_typed_serialize(
    response: &ReadRecordsResponse,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.get_catalog_name()))?;
    jgen.write_field_name(RECORDS_FIELD)?;
    super::block_serde::serialize(Some(response.get_records()), jgen, version)
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<ReadRecordsResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(RECORDS_FIELD))?;
    let records = super::block_serde::deserialize(jparser, ctx)?;
    Ok(ReadRecordsResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(records, "records is null")?,
    ))
}
