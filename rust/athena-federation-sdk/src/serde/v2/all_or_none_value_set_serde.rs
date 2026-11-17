use crate::domain::predicate::AllOrNoneValueSet;
use crate::exceptions::Result;
use crate::serde::base::{assert_field_name, get_next_bool_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};

const TYPE_FIELD: &str = "type";
const ALL_FIELD: &str = "all";
const NULL_ALLOWED_FIELD: &str = "nullAllowed";

pub fn do_typed_serialize(value_set: &AllOrNoneValueSet, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_field_name(TYPE_FIELD)?;
    super::arrow_type_serde::serialize(Some(value_set.get_type()), jgen)?;
    jgen.write_boolean_field(ALL_FIELD, value_set.is_all())?;
    jgen.write_boolean_field(NULL_ALLOWED_FIELD, value_set.is_null_allowed())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<AllOrNoneValueSet> {
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    let arrow_type = super::arrow_type_serde::deserialize(jparser)?;
    let all = get_next_bool_field(jparser, ALL_FIELD)?;
    let null_allowed = get_next_bool_field(jparser, NULL_ALLOWED_FIELD)?;
    Ok(AllOrNoneValueSet::new(require_non_null(arrow_type, "type is null")?, all, null_allowed))
}
