use indexmap::IndexMap;

use crate::exceptions::{FederationError, Result};
use crate::metadata::optimizations::OptimizationSubType;
use crate::metadata::GetDataSourceCapabilitiesResponse;
use crate::serde::base::{
    assert_field_name, get_next_string_field, require_non_null, validate_object_end,
    validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

const CATALOG_NAME_FIELD: &str = "catalogName";
const CAPABILITIES_NAME_FIELD: &str = "capabilities";

pub fn do_typed_serialize(
    response: &GetDataSourceCapabilitiesResponse,
    jgen: &mut JsonGenerator,
) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.get_catalog_name()))?;
    jgen.write_object_field_start(CAPABILITIES_NAME_FIELD)?;
    for (name, sub_types) in response.get_capabilities() {
        jgen.write_array_field_start(name)?;
        for sub_type in sub_types {
            super::optimization_sub_type_serde_v4::serialize(Some(sub_type), jgen)?;
        }
        jgen.write_end_array()?;
    }
    jgen.write_end_object()
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
) -> Result<GetDataSourceCapabilitiesResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(CAPABILITIES_NAME_FIELD))?;
    jparser.next_token()?;
    let mut capabilities: IndexMap<String, Vec<OptimizationSubType>> = IndexMap::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndObject) => break,
            None => {
                return Err(FederationError::Io(
                    "Unexpected end-of-input: expected close marker for OBJECT".into(),
                ))
            }
            Some(_) => {
                let field = jparser.current_name().unwrap_or_default();
                jparser.next_token()?;
                let mut sub_types = Vec::new();
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
                            sub_types.push(super::optimization_sub_type_serde_v4::do_deserialize(
                                jparser,
                            )?);
                            validate_object_end(jparser)?;
                        }
                    }
                }
                if capabilities.contains_key(&field) {
                    return Err(FederationError::IllegalArgument(format!(
                        "Multiple entries with same key: {field}"
                    )));
                }
                capabilities.insert(field, sub_types);
            }
        }
    }
    Ok(GetDataSourceCapabilitiesResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        capabilities,
    ))
}
