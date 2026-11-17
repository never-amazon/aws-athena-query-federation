use std::collections::HashMap;

use crate::exceptions::Result;
use crate::metadata::GetTableRequest;
use crate::serde::base::{assert_field_name, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};

const TABLE_NAME_FIELD: &str = "tableName";

pub fn do_typed_serialize(request: &GetTableRequest, jgen: &mut JsonGenerator) -> Result<()> {
    super::metadata_request_serde::serialize_header(request, jgen)?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(request.get_table_name(), jgen)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<GetTableRequest> {
    let header = super::metadata_request_serde::deserialize_header(jparser)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    let catalog_name = header.catalog_name()?;
    Ok(GetTableRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
        require_non_null(table_name, "tableName is null")?,
        HashMap::new(),
    ))
}
