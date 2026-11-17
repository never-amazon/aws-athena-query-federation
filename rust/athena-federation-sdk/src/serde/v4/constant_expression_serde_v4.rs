use crate::domain::predicate::expression::ConstantExpression;
use crate::exceptions::Result;
use crate::serde::base::{assert_field_name, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const VALUE_BLOCK_FIELD: &str = "valueBlock";
const TYPE_FIELD: &str = "type";

pub fn do_typed_serialize(
    expression: &ConstantExpression,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    jgen.write_field_name(VALUE_BLOCK_FIELD)?;
    crate::serde::v2::block_serde::serialize(expression.values(), jgen, version)?;
    jgen.write_field_name(TYPE_FIELD)?;
    crate::serde::v2::arrow_type_serde::serialize(Some(expression.get_type()), jgen)
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<ConstantExpression> {
    assert_field_name(jparser, Some(VALUE_BLOCK_FIELD))?;
    let value_block = crate::serde::v2::block_serde::deserialize(jparser, ctx)?;
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    let arrow_type = crate::serde::v2::arrow_type_serde::deserialize(jparser)?;
    Ok(ConstantExpression::new(
        value_block,
        require_non_null(arrow_type, "type is null")?,
    ))
}
