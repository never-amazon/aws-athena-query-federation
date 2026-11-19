use crate::domain::predicate::{Bound, Marker};
use crate::exceptions::Result;
use crate::serde::base::{
    assert_field_name, deserialize_nullable, get_next_bool_field, get_next_string_field,
    no_enum_constant, require_non_null, serialize_object,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::{SerDeContext, SerdeVersion};

const VALUE_BLOCK_FIELD: &str = "valueBlock";
const BOUND_FIELD: &str = "bound";
const NULL_VALUE_FIELD: &str = "nullValue";

pub fn bound_value_of(name: Option<&str>) -> Result<Bound> {
    match name {
        Some("BELOW") => Ok(Bound::Below),
        Some("EXACTLY") => Ok(Bound::Exactly),
        Some("ABOVE") => Ok(Bound::Above),
        other => Err(no_enum_constant(
            "com.amazonaws.athena.connector.lambda.domain.predicate.Marker.Bound",
            other,
        )),
    }
}

pub fn serialize(marker: &Marker, jgen: &mut JsonGenerator, version: SerdeVersion) -> Result<()> {
    serialize_object(jgen, |j| do_serialize(marker, j, version))
}

pub fn do_serialize(
    marker: &Marker,
    jgen: &mut JsonGenerator,
    version: SerdeVersion,
) -> Result<()> {
    jgen.write_field_name(VALUE_BLOCK_FIELD)?;
    let block = marker.get_value_block()?;
    super::block_serde::serialize(Some(&block), jgen, version)?;
    jgen.write_string_field(BOUND_FIELD, Some(marker.bound().as_str()))?;
    jgen.write_boolean_field(NULL_VALUE_FIELD, marker.is_null_value())
}

pub fn deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Option<Marker>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<Marker> {
    assert_field_name(jparser, Some(VALUE_BLOCK_FIELD))?;
    let value_block = super::block_serde::deserialize(jparser, ctx)?;
    let bound = bound_value_of(get_next_string_field(jparser, BOUND_FIELD)?.as_deref())?;
    let null_value = get_next_bool_field(jparser, NULL_VALUE_FIELD)?;
    let value_block = require_non_null(value_block, "valueBlock is null")?;
    Marker::from_value_block(&value_block, bound, null_value)
}
