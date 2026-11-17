use crate::exceptions::Result;
use crate::metadata::GetTableLayoutResponse;
use crate::serde::base::{assert_field_name, get_next_string_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const CATALOG_NAME_FIELD: &str = "catalogName";
const TABLE_NAME_FIELD: &str = "tableName";
const PARTITIONS_FIELD: &str = "partitions";

pub fn do_typed_serialize(response: &GetTableLayoutResponse, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.get_catalog_name()))?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(response.get_table_name(), jgen)?;
    jgen.write_field_name(PARTITIONS_FIELD)?;
    super::block_serde::serialize(Some(response.get_partitions()), jgen, version)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<GetTableLayoutResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(PARTITIONS_FIELD))?;
    let partitions = super::block_serde::deserialize(jparser, ctx)?;
    Ok(GetTableLayoutResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(table_name, "tableName is null")?,
        require_non_null(partitions, "partitions is null")?,
    ))
}
