use crate::domain::spill::{S3SpillLocation, SpillLocation};
use crate::exceptions::Result;
use crate::serde::base::{get_next_bool_field, get_next_string_field, require_non_null};
use crate::serde::json::{JsonGenerator, JsonParser};

const BUCKET_FIELD: &str = "bucket";
const KEY_FIELD: &str = "key";
const DIRECTORY_FIELD: &str = "directory";

pub fn do_typed_serialize(spill_location: &S3SpillLocation, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(BUCKET_FIELD, Some(spill_location.get_bucket()))?;
    jgen.write_string_field(KEY_FIELD, Some(spill_location.get_key()))?;
    jgen.write_boolean_field(DIRECTORY_FIELD, spill_location.is_directory())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<SpillLocation> {
    let bucket = get_next_string_field(jparser, BUCKET_FIELD)?;
    let key = get_next_string_field(jparser, KEY_FIELD)?;
    let directory = get_next_bool_field(jparser, DIRECTORY_FIELD)?;
    Ok(SpillLocation::S3(S3SpillLocation::new(
        require_non_null(bucket, "bucket is null")?,
        require_non_null(key, "key is null")?,
        directory,
    )))
}
