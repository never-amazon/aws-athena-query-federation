use crate::domain::predicate::Range;
use crate::exceptions::Result;
use crate::serde::base::{
    assert_field_name, deserialize_nullable, require_non_null, serialize_object,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

const LOW_FIELD: &str = "low";
const HIGH_FIELD: &str = "high";

pub fn serialize(range: &Range, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    serialize_object(jgen, |j| do_serialize(range, j, version))
}

pub fn do_serialize(range: &Range, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    jgen.write_field_name(LOW_FIELD)?;
    super::marker_serde::serialize(range.get_low(), jgen, version)?;
    jgen.write_field_name(HIGH_FIELD)?;
    super::marker_serde::serialize(range.get_high(), jgen, version)
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Option<Range>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Range> {
    assert_field_name(jparser, Some(LOW_FIELD))?;
    let low = super::marker_serde::deserialize(jparser, ctx)?;
    assert_field_name(jparser, Some(HIGH_FIELD))?;
    let high = super::marker_serde::deserialize(jparser, ctx)?;
    Range::new(
        require_non_null(low, "low value is null")?,
        require_non_null(high, "high value is null")?,
    )
}
