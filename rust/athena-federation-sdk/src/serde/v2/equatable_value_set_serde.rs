use crate::domain::predicate::EquatableValueSet;
use crate::exceptions::Result;
use crate::serde::base::{assert_field_name, get_next_bool_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const VALUE_BLOCK_FIELD: &str = "valueBlock";
const ALLOW_LIST_FIELD: &str = "whiteList";
const NULL_ALLOWED_FIELD: &str = "nullAllowed";

pub fn do_typed_serialize(
    value_set: &EquatableValueSet,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    jgen.write_field_name(VALUE_BLOCK_FIELD)?;
    super::block_serde::serialize(Some(value_set.value_block()), jgen, version)?;
    jgen.write_boolean_field(ALLOW_LIST_FIELD, value_set.is_allow_list())?;
    jgen.write_boolean_field(NULL_ALLOWED_FIELD, value_set.is_null_allowed())
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<EquatableValueSet> {
    assert_field_name(jparser, Some(VALUE_BLOCK_FIELD))?;
    let value_block = super::block_serde::deserialize(jparser, ctx)?;
    let allow_list = get_next_bool_field(jparser, ALLOW_LIST_FIELD)?;
    let null_allowed = get_next_bool_field(jparser, NULL_ALLOWED_FIELD)?;
    Ok(EquatableValueSet::new(
        require_non_null(value_block, "valueBlock is null")?,
        allow_list,
        null_allowed,
    ))
}
