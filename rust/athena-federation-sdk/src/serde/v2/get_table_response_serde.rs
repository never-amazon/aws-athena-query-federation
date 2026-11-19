use indexmap::IndexSet;

use crate::exceptions::Result;
use crate::metadata::GetTableResponse;
use crate::serde::base::{
    assert_field_name, get_next_string_array, get_next_string_field, non_null_strings,
    require_non_null, write_string_array,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SerdeVersion};

const CATALOG_NAME_FIELD: &str = "catalogName";
const TABLE_NAME_FIELD: &str = "tableName";
const SCHEMA_FIELD: &str = "schema";
const PARTITION_COLUMNS_FIELD: &str = "partitionColumns";

pub fn do_typed_serialize(
    response: &GetTableResponse,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.catalog_name()))?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(response.table_name(), jgen)?;
    jgen.write_field_name(SCHEMA_FIELD)?;
    crate::serde::v3::schema_serde_v3::serialize_for_version(response.schema(), jgen, version)?;
    write_string_array(jgen, PARTITION_COLUMNS_FIELD, response.partition_columns())
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<GetTableResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(SCHEMA_FIELD))?;
    let schema = super::block_serde::deserialize_schema_for_version(jparser, ctx.version)?;
    let partition_cols: IndexSet<String> = non_null_strings(
        get_next_string_array(jparser, PARTITION_COLUMNS_FIELD)?,
        "null element in partitionColumns",
    )?
    .into_iter()
    .collect();
    Ok(GetTableResponse::new(
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(table_name, "tableName is null")?,
        schema,
        partition_cols,
    ))
}
