use crate::domain::predicate::expression::VariableExpression;
use crate::exceptions::Result;
use crate::serde::base::{assert_field_name, get_next_string_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};

const COLUMN_NAME_FIELD: &str = "columnName";
const TYPE_FIELD: &str = "type";

pub fn do_typed_serialize(expression: &VariableExpression, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(COLUMN_NAME_FIELD, Some(expression.column_name()))?;
    jgen.write_field_name(TYPE_FIELD)?;
    crate::serde::v2::arrow_type_serde::serialize(Some(expression.get_type()), jgen)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<VariableExpression> {
    let column_name = get_next_string_field(jparser, COLUMN_NAME_FIELD)?;
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    let arrow_type = crate::serde::v2::arrow_type_serde::deserialize(jparser)?;
    VariableExpression::new(
        require_non_null(column_name, "columnName is null")?,
        require_non_null(arrow_type, "type is null")?,
    )
}
