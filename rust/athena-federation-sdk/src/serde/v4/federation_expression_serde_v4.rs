use crate::domain::predicate::expression::FederationExpression;
use crate::exceptions::Result;
use crate::serde::base::{deserialize_nullable, get_type, no_serde_configured, serialize_nullable, write_type};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

pub fn serialize(expression: Option<&FederationExpression>, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    serialize_nullable(jgen, expression, |e, j| do_serialize(e, j, version))
}

pub fn do_serialize(expression: &FederationExpression, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    match expression {
        FederationExpression::Constant(c) => {
            write_type(jgen, "ConstantExpression")?;
            super::constant_expression_serde_v4::do_typed_serialize(c, jgen, version)
        }
        FederationExpression::FunctionCall(f) => {
            write_type(jgen, "FunctionCallExpression")?;
            super::function_call_expression_serde_v4::do_typed_serialize(f, jgen, version)
        }
        FederationExpression::Variable(v) => {
            write_type(jgen, "VariableExpression")?;
            super::variable_expression_serde_v4::do_typed_serialize(v, jgen)
        }
    }
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Option<FederationExpression>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<FederationExpression> {
    let type_name = get_type(jparser)?;
    match type_name.as_deref() {
        Some("ConstantExpression") => super::constant_expression_serde_v4::do_typed_deserialize(jparser, ctx).map(FederationExpression::from),
        Some("FunctionCallExpression") => {
            super::function_call_expression_serde_v4::do_typed_deserialize(jparser, ctx).map(FederationExpression::from)
        }
        Some("VariableExpression") => super::variable_expression_serde_v4::do_typed_deserialize(jparser).map(FederationExpression::from),
        other => Err(no_serde_configured(other.unwrap_or("null"))),
    }
}
