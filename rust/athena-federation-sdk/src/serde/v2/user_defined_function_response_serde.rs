use crate::exceptions::Result;
use crate::serde::base::{assert_field_name, get_next_string_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SerdeVersion};
use crate::udf::UserDefinedFunctionResponse;

const RECORDS_FIELD: &str = "records";
const METHOD_NAME_FIELD: &str = "methodName";

pub fn do_typed_serialize(
    response: &UserDefinedFunctionResponse,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_field_name(RECORDS_FIELD)?;
    super::block_serde::serialize(Some(response.records()), jgen, version)?;
    jgen.write_string_field(METHOD_NAME_FIELD, Some(response.method_name()))
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<UserDefinedFunctionResponse> {
    assert_field_name(jparser, Some(RECORDS_FIELD))?;
    let records = super::block_serde::deserialize(jparser, ctx)?;
    let method_name = get_next_string_field(jparser, METHOD_NAME_FIELD)?;
    Ok(UserDefinedFunctionResponse::new(
        require_non_null(records, "records is null")?,
        require_non_null(method_name, "methodName is null")?,
    ))
}
