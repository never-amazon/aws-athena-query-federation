use crate::domain::predicate::ValueSet;
use crate::exceptions::Result;
use crate::serde::base::{
    deserialize_nullable, get_type, no_serde_configured, serialize_nullable, write_type,
};
use crate::serde::json::{JsonGenerator, JsonParser};
use crate::serde::SerDeContext;

pub const SERDE_SUB_TYPES: &[&str] = &["EquatableValueSet", "SortedRangeSet", "AllOrNoneValueSet"];

pub fn serialize(
    value_set: Option<&ValueSet>,
    jgen: &mut JsonGenerator,
    version: i32,
) -> Result<()> {
    serialize_nullable(jgen, value_set, |v, j| do_serialize(v, j, version))
}

pub fn do_serialize(value_set: &ValueSet, jgen: &mut JsonGenerator, version: i32) -> Result<()> {
    match value_set {
        ValueSet::EquatableValueSet(v) => {
            write_type(jgen, "EquatableValueSet")?;
            super::equatable_value_set_serde::do_typed_serialize(v, jgen, version)
        }
        ValueSet::SortedRangeSet(v) => {
            write_type(jgen, "SortedRangeSet")?;
            super::sorted_range_set_serde::do_typed_serialize(v, jgen, version)
        }
        ValueSet::AllOrNoneValueSet(v) => {
            write_type(jgen, "AllOrNoneValueSet")?;
            super::all_or_none_value_set_serde::do_typed_serialize(v, jgen)
        }
    }
}

pub fn deserialize(
    jparser: &mut JsonParser<'_>,
    ctx: &SerDeContext<'_>,
) -> Result<Option<ValueSet>> {
    deserialize_nullable(jparser, |p| do_deserialize(p, ctx))
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>, ctx: &SerDeContext<'_>) -> Result<ValueSet> {
    let type_name = get_type(jparser)?;
    match type_name.as_deref() {
        Some("EquatableValueSet") => {
            super::equatable_value_set_serde::do_typed_deserialize(jparser, ctx).map(ValueSet::from)
        }
        Some("SortedRangeSet") => {
            super::sorted_range_set_serde::do_typed_deserialize(jparser, ctx).map(ValueSet::from)
        }
        Some("AllOrNoneValueSet") => {
            super::all_or_none_value_set_serde::do_typed_deserialize(jparser).map(ValueSet::from)
        }
        other => Err(no_serde_configured(other.unwrap_or("null"))),
    }
}
