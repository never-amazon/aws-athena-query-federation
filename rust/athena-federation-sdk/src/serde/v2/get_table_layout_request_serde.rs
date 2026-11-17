use indexmap::IndexSet;

use crate::exceptions::Result;
use crate::metadata::GetTableLayoutRequest;
use crate::serde::base::{
    assert_field_name, get_next_string_array, non_null_strings, require_non_null,
    write_string_array,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const TABLE_NAME_FIELD: &str = "tableName";
const CONSTRAINTS_FIELD: &str = "constraints";
const SCHEMA_FIELD: &str = "schema";
const PARTITION_COLS_FIELD: &str = "partitionColumns";

pub fn do_typed_serialize(
    request: &GetTableLayoutRequest,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    super::metadata_request_serde::serialize_header(request, jgen)?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(request.table_name(), jgen)?;
    jgen.write_field_name(CONSTRAINTS_FIELD)?;
    super::constraints_serde::serialize(Some(request.constraints()), jgen, version)?;
    jgen.write_field_name(SCHEMA_FIELD)?;
    crate::serde::v3::schema_serde_v3::serialize_for_version(request.schema(), jgen, version)?;
    write_string_array(jgen, PARTITION_COLS_FIELD, request.partition_cols())
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<GetTableLayoutRequest> {
    let header = super::metadata_request_serde::deserialize_header(jparser)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(CONSTRAINTS_FIELD))?;
    let constraints = super::constraints_serde::deserialize(jparser, ctx)?;
    assert_field_name(jparser, Some(SCHEMA_FIELD))?;
    let schema = super::block_serde::deserialize_schema_for_version(jparser, ctx.version)?;
    let partition_cols = non_null_strings(
        get_next_string_array(jparser, PARTITION_COLS_FIELD)?,
        "null element in partitionColumns",
    )?;
    let partition_cols: IndexSet<String> = partition_cols.into_iter().collect();
    let catalog_name = header.catalog_name()?;
    Ok(GetTableLayoutRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
        require_non_null(table_name, "tableName is null")?,
        require_non_null(constraints, "constraints is null")?,
        schema,
        partition_cols,
    ))
}
