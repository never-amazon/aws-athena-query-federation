use crate::domain::predicate::functions::FunctionName;
use crate::exceptions::Result;
use crate::serde::base::{
    deserialize_nullable, get_next_string_field, require_non_null, serialize_nullable,
};
use crate::serde::json::{JsonGenerator, JsonParser};

const FUNCTION_NAME_FIELD: &str = "functionName";

pub fn serialize(function_name: Option<&FunctionName>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, function_name, do_serialize)
}

pub fn do_serialize(function_name: &FunctionName, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(FUNCTION_NAME_FIELD, Some(function_name.function_name()))
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<FunctionName>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<FunctionName> {
    let function_name = get_next_string_field(jparser, FUNCTION_NAME_FIELD)?;
    Ok(FunctionName::new(require_non_null(
        function_name,
        "functionName is null",
    )?))
}
