use std::collections::HashMap;

use crate::exceptions::Result;
use crate::metadata::GetTableRequest;
use crate::serde::base::{assert_field_name, read_string_map_entries, require_non_null, validate_object_start_token, write_string_map};
use crate::serde::json::{JsonGenerator, JsonParser};

const TABLE_NAME_FIELD: &str = "tableName";
const QUERY_PASSTHROUGH_ARGUMENTS: &str = "queryPassthroughArguments";

pub fn do_typed_serialize(request: &GetTableRequest, jgen: &mut JsonGenerator) -> Result<()> {
    crate::serde::v2::metadata_request_serde::serialize_header(request, jgen)?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    crate::serde::v2::table_name_serde::serialize(request.get_table_name(), jgen)?;
    write_string_map(jgen, QUERY_PASSTHROUGH_ARGUMENTS, request.get_query_passthrough_arguments())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<GetTableRequest> {
    let header = crate::serde::v2::metadata_request_serde::deserialize_header(jparser)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = crate::serde::v2::table_name_serde::deserialize(jparser)?;
    let mut query_passthrough_arguments = HashMap::new();
    assert_field_name(jparser, Some(QUERY_PASSTHROUGH_ARGUMENTS))?;
    validate_object_start_token(jparser.next_token()?)?;
    for (k, v) in read_string_map_entries(jparser)? {
        query_passthrough_arguments.insert(k, v);
    }
    let catalog_name = header.catalog_name()?;
    Ok(GetTableRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
        require_non_null(table_name, "tableName is null")?,
        query_passthrough_arguments,
    ))
}
