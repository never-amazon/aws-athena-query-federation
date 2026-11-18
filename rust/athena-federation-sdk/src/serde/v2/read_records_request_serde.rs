use crate::exceptions::Result;
use crate::records::ReadRecordsRequest;
use crate::serde::base::{
    assert_field_name, get_next_string_field, parse_java_long, require_non_null,
};
use crate::serde::federated_identity_serde;
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SerdeVersion};

const IDENTITY_FIELD: &str = "identity";
const QUERY_ID_FIELD: &str = "queryId";
const CATALOG_NAME_FIELD: &str = "catalogName";
const TABLE_NAME_FIELD: &str = "tableName";
const SCHEMA_FIELD: &str = "schema";
const SPLIT_FIELD: &str = "split";
const CONSTRAINTS_FIELD: &str = "constraints";
const MAX_BLOCK_SIZE_FIELD: &str = "maxBlockSize";
const MAX_INLINE_BLOCK_SIZE_FIELD: &str = "maxInlineBlockSize";

pub fn do_typed_serialize(
    request: &ReadRecordsRequest,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_field_name(IDENTITY_FIELD)?;
    federated_identity_serde::serialize(request.identity(), jgen)?;
    jgen.write_string_field(QUERY_ID_FIELD, Some(request.query_id()))?;
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(request.catalog_name()))?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(request.table_name(), jgen)?;
    jgen.write_field_name(SCHEMA_FIELD)?;
    crate::serde::v3::schema_serde_v3::serialize_for_version(request.schema(), jgen, version)?;
    jgen.write_field_name(SPLIT_FIELD)?;
    super::split_serde::serialize(Some(request.split()), jgen)?;
    jgen.write_field_name(CONSTRAINTS_FIELD)?;
    super::constraints_serde::serialize(Some(request.constraints()), jgen, version)?;
    jgen.write_string_field(
        MAX_BLOCK_SIZE_FIELD,
        Some(&request.max_block_size().to_string()),
    )?;
    jgen.write_string_field(
        MAX_INLINE_BLOCK_SIZE_FIELD,
        Some(&request.max_inline_block_size().to_string()),
    )
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<ReadRecordsRequest> {
    assert_field_name(jparser, Some(IDENTITY_FIELD))?;
    let identity = federated_identity_serde::deserialize(jparser)?;
    let query_id = get_next_string_field(jparser, QUERY_ID_FIELD)?;
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(SCHEMA_FIELD))?;
    let schema = super::block_serde::deserialize_schema_for_version(jparser, ctx.version)?;
    assert_field_name(jparser, Some(SPLIT_FIELD))?;
    let split = super::split_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(CONSTRAINTS_FIELD))?;
    let constraints = super::constraints_serde::deserialize(jparser, ctx)?;
    let max_block_size =
        parse_java_long(get_next_string_field(jparser, MAX_BLOCK_SIZE_FIELD)?.as_deref())?;
    let max_inline_block_size =
        parse_java_long(get_next_string_field(jparser, MAX_INLINE_BLOCK_SIZE_FIELD)?.as_deref())?;
    Ok(ReadRecordsRequest::new(
        identity,
        require_non_null(catalog_name, "catalogName is null")?,
        require_non_null(query_id, "queryId is null")?,
        require_non_null(table_name, "tableName is null")?,
        schema,
        require_non_null(split, "split is null")?,
        require_non_null(constraints, "constraints is null")?,
        max_block_size,
        max_inline_block_size,
    ))
}
