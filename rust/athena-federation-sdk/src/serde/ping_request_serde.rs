use crate::exceptions::Result;
use crate::request::PingRequest;
use crate::serde::base::{
    assert_field_name, get_next_string_field, require_non_null, validate_object_start_token,
};
use crate::serde::federated_identity_serde;
use crate::serde::json::{JsonGenerator, JsonParser};

const IDENTITY_FIELD: &str = "identity";
const CATALOG_NAME_FIELD: &str = "catalogName";
const QUERY_ID_FIELD: &str = "queryId";

pub fn do_typed_serialize(request: &PingRequest, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_field_name(IDENTITY_FIELD)?;
    federated_identity_serde::serialize(request.identity(), jgen)?;
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(request.catalog_name()))?;
    jgen.write_string_field(QUERY_ID_FIELD, Some(request.query_id()))
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<PingRequest> {
    validate_object_start_token(jparser.current_token())?;
    crate::serde::base::get_type(jparser)?;
    do_typed_deserialize(jparser)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<PingRequest> {
    assert_field_name(jparser, Some(IDENTITY_FIELD))?;
    let identity = federated_identity_serde::deserialize(jparser)?;
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    let query_id = get_next_string_field(jparser, QUERY_ID_FIELD)?;
    Ok(PingRequest::new(
        identity,
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(query_id, "queryId is null")?,
    ))
}
