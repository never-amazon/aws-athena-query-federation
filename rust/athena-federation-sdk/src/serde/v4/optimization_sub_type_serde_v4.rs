use crate::exceptions::Result;
use crate::metadata::optimizations::OptimizationSubType;
use crate::serde::base::{
    deserialize_nullable, get_next_string_array, get_next_string_field, non_null_strings,
    require_non_null, serialize_nullable, write_string_array,
};
use crate::serde::json::{JsonGenerator, JsonParser};

const SUBTYPE_FIELD: &str = "subType";
const PROPERTIES_FIELD: &str = "properties";

pub fn serialize(sub_type: Option<&OptimizationSubType>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, sub_type, do_serialize)
}

pub fn do_serialize(sub_type: &OptimizationSubType, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(SUBTYPE_FIELD, Some(sub_type.sub_type()))?;
    write_string_array(jgen, PROPERTIES_FIELD, sub_type.properties())
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<OptimizationSubType>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<OptimizationSubType> {
    let optimization = get_next_string_field(jparser, SUBTYPE_FIELD)?;
    let properties = non_null_strings(
        get_next_string_array(jparser, PROPERTIES_FIELD)?,
        "null element in properties",
    )?;
    Ok(OptimizationSubType::new(
        require_non_null(optimization, "subType is null")?,
        properties,
    ))
}
