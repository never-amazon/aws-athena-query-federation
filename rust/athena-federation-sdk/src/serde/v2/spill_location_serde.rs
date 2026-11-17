use crate::domain::spill::SpillLocation;
use crate::exceptions::Result;
use crate::serde::base::{
    deserialize_nullable, get_type, no_serde_configured, serialize_nullable, write_type,
};
use crate::serde::json::{JsonGenerator, JsonParser};

pub fn serialize(spill_location: Option<&SpillLocation>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, spill_location, do_serialize)
}

pub fn do_serialize(spill_location: &SpillLocation, jgen: &mut JsonGenerator) -> Result<()> {
    match spill_location {
        SpillLocation::S3(s3) => {
            write_type(jgen, "S3SpillLocation")?;
            super::s3_spill_location_serde::do_typed_serialize(s3, jgen)
        }
    }
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<SpillLocation>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<SpillLocation> {
    let type_name = get_type(jparser)?;
    match type_name.as_deref() {
        Some("S3SpillLocation") => super::s3_spill_location_serde::do_typed_deserialize(jparser),
        other => Err(no_serde_configured(other.unwrap_or("null"))),
    }
}
