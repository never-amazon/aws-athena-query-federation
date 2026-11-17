use crate::domain::table_name::TableName;
use crate::exceptions::Result;
use crate::serde::base::{
    deserialize_nullable, get_next_string_field, require_non_null, serialize_object,
};
use crate::serde::json::{JsonGenerator, JsonParser};

const SCHEMA_NAME_FIELD: &str = "schemaName";
const TABLE_NAME_FIELD: &str = "tableName";

pub fn serialize(table_name: &TableName, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_object(jgen, |j| do_serialize(table_name, j))
}

pub fn do_serialize(table_name: &TableName, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(SCHEMA_NAME_FIELD, Some(table_name.schema_name()))?;
    jgen.write_string_field(TABLE_NAME_FIELD, Some(table_name.table_name()))
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<TableName>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<TableName> {
    let schema_name = get_next_string_field(jparser, SCHEMA_NAME_FIELD)?;
    let table_name = get_next_string_field(jparser, TABLE_NAME_FIELD)?;
    Ok(TableName::new(
        require_non_null(schema_name, "schemaName is null")?,
        require_non_null(table_name, "tableName is null")?,
    ))
}
