use crate::exceptions::{FederationError, Result};
use crate::metadata::GetSplitsResponse;
use crate::serde::base::{
    assert_field_name, get_next_string_field, require_non_null, validate_array_start,
    validate_object_end, validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

const CATALOG_NAME_FIELD: &str = "catalogName";
const SPLITS_FIELD: &str = "splits";
const CONTINUATION_TOKEN_FIELD: &str = "continuationToken";

pub fn do_typed_serialize(response: &GetSplitsResponse, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.get_catalog_name()))?;
    jgen.write_array_field_start(SPLITS_FIELD)?;
    for split in response.get_splits() {
        super::split_serde::serialize(Some(split), jgen)?;
    }
    jgen.write_end_array()?;
    jgen.write_string_field(CONTINUATION_TOKEN_FIELD, response.get_continuation_token())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<GetSplitsResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(SPLITS_FIELD))?;
    validate_array_start(jparser)?;
    let mut splits = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::Io(
                    "Unexpected end-of-input: expected close marker for ARRAY".into(),
                ))
            }
            token => {
                validate_object_start_token(token)?;
                splits.push(super::split_serde::do_deserialize(jparser)?);
                validate_object_end(jparser)?;
            }
        }
    }
    let continuation_token = get_next_string_field(jparser, CONTINUATION_TOKEN_FIELD)?;
    Ok(GetSplitsResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        splits,
        continuation_token,
    ))
}
