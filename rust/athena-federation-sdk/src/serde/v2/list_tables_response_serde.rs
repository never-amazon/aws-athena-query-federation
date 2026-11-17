use crate::exceptions::{FederationError, Result};
use crate::metadata::ListTablesResponse;
use crate::serde::base::{
    assert_field_name, get_next_string_field, require_non_null, validate_array_start,
    validate_object_end, validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

const TABLES_FIELD: &str = "tables";
const CATALOG_NAME_FIELD: &str = "catalogName";
const NEXT_TOKEN_FIELD: &str = "nextToken";

pub fn do_typed_serialize(response: &ListTablesResponse, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_array_field_start(TABLES_FIELD)?;
    for table_name in response.tables() {
        super::table_name_serde::serialize(table_name, jgen)?;
    }
    jgen.write_end_array()?;
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.catalog_name()))?;
    jgen.write_string_field(NEXT_TOKEN_FIELD, response.next_token())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<ListTablesResponse> {
    assert_field_name(jparser, Some(TABLES_FIELD))?;
    let mut tables = Vec::new();
    validate_array_start(jparser)?;
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
                tables.push(super::table_name_serde::do_deserialize(jparser)?);
                validate_object_end(jparser)?;
            }
        }
    }
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    let mut next_token = None;
    if jparser.next_token()? != Some(JsonToken::EndObject) {
        let name = jparser
            .current_name()
            .ok_or_else(|| FederationError::NullPointer("Cannot invoke \"String.equals(Object)\" because the return value of \"getCurrentName()\" is null".into()))?;
        if name == NEXT_TOKEN_FIELD {
            jparser.next_token()?;
            next_token = jparser.get_value_as_string();
        }
    }
    Ok(ListTablesResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        tables,
        next_token,
    ))
}
