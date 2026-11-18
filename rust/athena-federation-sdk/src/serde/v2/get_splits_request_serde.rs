use crate::exceptions::Result;
use crate::metadata::GetSplitsRequest;
use crate::serde::base::{
    assert_field_name, get_next_string_array, get_next_string_field, non_null_strings,
    require_non_null, write_string_array,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SerdeVersion};

const TABLE_NAME_FIELD: &str = "tableName";
const PARTITIONS_FIELD: &str = "partitions";
const PARTITION_COLS_FIELD: &str = "partitionColumns";
const CONSTRAINTS_FIELD: &str = "constraints";
const CONTINUATION_TOKEN_FIELD: &str = "continuationToken";

pub fn do_typed_serialize(
    request: &GetSplitsRequest,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    super::metadata_request_serde::serialize_header(request, jgen)?;
    jgen.write_field_name(TABLE_NAME_FIELD)?;
    super::table_name_serde::serialize(request.table_name(), jgen)?;
    jgen.write_field_name(PARTITIONS_FIELD)?;
    super::block_serde::serialize(Some(request.partitions()), jgen, version)?;
    write_string_array(jgen, PARTITION_COLS_FIELD, request.partition_cols())?;
    jgen.write_field_name(CONSTRAINTS_FIELD)?;
    super::constraints_serde::serialize(Some(request.constraints()), jgen, version)?;
    jgen.write_string_field(CONTINUATION_TOKEN_FIELD, request.continuation_token())
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<GetSplitsRequest> {
    let header = super::metadata_request_serde::deserialize_header(jparser)?;
    assert_field_name(jparser, Some(TABLE_NAME_FIELD))?;
    let table_name = super::table_name_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(PARTITIONS_FIELD))?;
    let partitions = super::block_serde::deserialize(jparser, ctx)?;
    let partition_columns = non_null_strings(
        get_next_string_array(jparser, PARTITION_COLS_FIELD)?,
        "null element in partitionColumns",
    )?;
    assert_field_name(jparser, Some(CONSTRAINTS_FIELD))?;
    let constraints = super::constraints_serde::deserialize(jparser, ctx)?;
    let continuation_token = get_next_string_field(jparser, CONTINUATION_TOKEN_FIELD)?;
    let catalog_name = header.catalog_name()?;
    Ok(GetSplitsRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
        require_non_null(table_name, "tableName is null")?,
        require_non_null(partitions, "partitions is null")?,
        partition_columns,
        require_non_null(constraints, "constraints is null")?,
        continuation_token,
    ))
}
