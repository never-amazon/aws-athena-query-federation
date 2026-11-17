use crate::exceptions::Result;
use crate::metadata::ListSchemasResponse;
use crate::serde::base::{get_next_string_array, get_next_string_field, non_null_strings, require_non_null, write_string_array};
use crate::serde::json::{JsonGenerator, JsonParser};

const CATALOG_NAME_FIELD: &str = "catalogName";
const SCHEMAS_FIELD: &str = "schemas";

pub fn do_typed_serialize(response: &ListSchemasResponse, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(response.get_catalog_name()))?;
    write_string_array(jgen, SCHEMAS_FIELD, response.get_schemas())
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<ListSchemasResponse> {
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    let schemas = non_null_strings(get_next_string_array(jparser, SCHEMAS_FIELD)?, "null element in schemas")?;
    Ok(ListSchemasResponse::new(require_non_null(catalog_name, "catalogName is null")?, schemas))
}
