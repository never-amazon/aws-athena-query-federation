use crate::domain::predicate::SortedRangeSet;
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{assert_field_name, get_next_bool_field, require_non_null, validate_array_start, validate_object_end, validate_object_start_token};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};
use crate::serde::SerDeContext;

const TYPE_FIELD: &str = "type";
const RANGES_FIELD: &str = "ranges";
const NULL_ALLOWED_FIELD: &str = "nullAllowed";

pub fn do_typed_serialize(value_set: &SortedRangeSet, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    jgen.write_field_name(TYPE_FIELD)?;
    super::arrow_type_serde::serialize(Some(value_set.get_type()), jgen)?;
    jgen.write_field_name(RANGES_FIELD)?;
    jgen.write_start_array()?;
    for range in value_set.ordered_ranges() {
        super::range_serde::serialize(range, jgen, version)?;
    }
    jgen.write_end_array()?;
    jgen.write_boolean_field(NULL_ALLOWED_FIELD, value_set.is_null_allowed())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<SortedRangeSet> {
    assert_field_name(jparser, Some(TYPE_FIELD))?;
    let arrow_type = super::arrow_type_serde::deserialize(jparser)?;
    assert_field_name(jparser, Some(RANGES_FIELD))?;
    validate_array_start(jparser)?;
    let mut ranges = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => return Err(FederationError::Io("Unexpected end-of-input: expected close marker for ARRAY".into())),
            token => {
                validate_object_start_token(token)?;
                ranges.push(super::range_serde::do_deserialize(jparser, ctx)?);
                validate_object_end(jparser)?;
            }
        }
    }
    let null_allowed = get_next_bool_field(jparser, NULL_ALLOWED_FIELD)?;
    let arrow_type = require_non_null(arrow_type, "type is null")?;
    SortedRangeSet::copy_of(&arrow_type, ranges, null_allowed)
}
