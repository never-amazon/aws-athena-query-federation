use crate::domain::predicate::expression::FunctionCallExpression;
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{
    assert_field_name, require_non_null, validate_array_start, validate_object_end,
    validate_object_start_token,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::{SerDeContext, SerdeVersion};

const FUNCTION_NAME_FIELD: &str = "functionName";
const TYPE_FIELD: &str = "type";
const ARGUMENTS_FIELD: &str = "arguments";

pub fn do_typed_serialize(
    expression: &FunctionCallExpression,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_field_name(TYPE_FIELD)?;
    crate::serde::v2::arrow_type_serde::serialize(Some(expression.get_type()), jgen)?;
    jgen.write_field_name(FUNCTION_NAME_FIELD)?;
    super::function_name_serde_v4::serialize(Some(expression.function_name()), jgen)?;
    jgen.write_array_field_start(ARGUMENTS_FIELD)?;
    for argument in expression.arguments() {
        super::federation_expression_serde_v4::serialize(Some(argument), jgen, version)?;
    }
    jgen.write_end_array()
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<FunctionCallExpression> {
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    let arrow_type = crate::serde::v2::arrow_type_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(FUNCTION_NAME_FIELD))?;
    let function_name = super::function_name_serde_v4::deserialize(jparser)?;
    assert_field_name(jparser, Some(ARGUMENTS_FIELD))?;
    validate_array_start(jparser)?;
    let mut arguments = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::Io(
                    "Unexpected end-of-input: expected close marker for ARRAY".into(),
                ))
            }
            token => {
                validate_object_start_token(token)?;
                arguments.push(super::federation_expression_serde_v4::do_deserialize(
                    jparser, ctx,
                )?);
                validate_object_end(jparser)?;
            }
        }
    }
    Ok(FunctionCallExpression::new(
        require_non_null(arrow_type, "type is null")?,
        require_non_null(function_name, "functionName is null")?,
        arguments,
    ))
}
