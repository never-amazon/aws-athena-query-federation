use crate::exceptions::{FederationError, Result};
use crate::request::PingResponse;
use crate::serde::base::{
    get_next_int_field, get_next_string_field, require_non_null, validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser};

const CATALOG_NAME_FIELD: &str = "catalogName";
const QUERY_ID_FIELD: &str = "queryId";
const SOURCE_TYPE_FIELD: &str = "sourceType";
const CAPABILITIES_FIELD: &str = "capabilities";
const SERDE_VERSION_FIELD: &str = "serDeVersion";

pub fn do_typed_serialize(response: &PingResponse, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.catalog_name()))?;
    jgen.write_string_field(QUERY_ID_FIELD, Some(response.query_id()))?;
    jgen.write_string_field(SOURCE_TYPE_FIELD, response.source_type())?;
    jgen.write_number_field_i32(CAPABILITIES_FIELD, response.capabilities())?;
    jgen.write_number_field_i32(SERDE_VERSION_FIELD, response.ser_de_version())
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<PingResponse> {
    validate_object_start_token(jparser.current_token())?;
    crate::serde::base::get_type(jparser)?;
    do_typed_deserialize(jparser)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<PingResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    let query_id = get_next_string_field(jparser, QUERY_ID_FIELD)?;
    let source_type = get_next_string_field(jparser, SOURCE_TYPE_FIELD)?;
    let capabilities = get_next_int_field(jparser, CAPABILITIES_FIELD)?;
    let ser_de_version = match get_next_int_field(jparser, SERDE_VERSION_FIELD) {
        Ok(v) => v,
        Err(FederationError::UnexpectedToken(_)) => 1,
        Err(e) => return Err(e),
    };
    Ok(PingResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(query_id, "queryId is null")?,
        source_type,
        capabilities,
        ser_de_version,
    ))
}
