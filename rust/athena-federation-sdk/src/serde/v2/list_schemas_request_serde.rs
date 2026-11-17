use crate::exceptions::Result;
use crate::metadata::ListSchemasRequest;
use crate::serde::json::{JsonGenerator, JsonParser};

pub fn do_typed_serialize(request: &ListSchemasRequest, jgen: &mut JsonGenerator) -> Result<()> {
    super::metadata_request_serde::serialize_header(request, jgen)
}

pub fn do_typed_deserialize(jparser: &mut JsonParser<'_>) -> Result<ListSchemasRequest> {
    let header = super::metadata_request_serde::deserialize_header(jparser)?;
    let catalog_name = header.catalog_name()?;
    Ok(ListSchemasRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
    ))
}
