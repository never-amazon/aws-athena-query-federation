use crate::exceptions::{FederationError, Result};
use crate::metadata::{ListTablesRequest, UNLIMITED_PAGE_SIZE_VALUE};
use crate::serde::base::{get_next_int_field, get_next_string_field};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

const SCHEMA_NAME_FIELD: &str = "schemaName";
const PAGE_SIZE_FIELD: &str = "pageSize";
const NEXT_TOKEN_FIELD: &str = "nextToken";

pub fn do_typed_serialize(request: &ListTablesRequest, jgen: &mut JsonGenerator) -> Result<()> {
    super::metadata_request_serde::serialize_header(request, jgen)?;
    jgen.write_string_field(SCHEMA_NAME_FIELD, request.get_schema_name())?;
    jgen.write_string_field(NEXT_TOKEN_FIELD, request.get_next_token())?;
    jgen.write_number_field_i32(PAGE_SIZE_FIELD, request.get_page_size())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<ListTablesRequest> {
    let header = super::metadata_request_serde::deserialize_header(jparser)?;
    let schema_name = get_next_string_field(jparser, SCHEMA_NAME_FIELD)?;
    let mut next_token = None;
    let mut page_size = UNLIMITED_PAGE_SIZE_VALUE;
    if jparser.next_token()? != Some(JsonToken::EndObject) {
        let name = jparser
            .current_name()
            .ok_or_else(|| FederationError::NullPointer("Cannot invoke \"String.equals(Object)\" because the return value of \"getCurrentName()\" is null".into()))?;
        if name == NEXT_TOKEN_FIELD {
            jparser.next_token()?;
            next_token = jparser.get_value_as_string();
            page_size = get_next_int_field(jparser, PAGE_SIZE_FIELD)?;
        }
    }
    let catalog_name = header.catalog_name()?;
    Ok(ListTablesRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
        schema_name,
        next_token,
        page_size,
    ))
}
