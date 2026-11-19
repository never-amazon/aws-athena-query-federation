use crate::exceptions::Result;
use crate::metadata::GetDataSourceCapabilitiesRequest;
use crate::serde::json::{JsonGenerator, JsonParser};

pub fn do_typed_serialize(
    request: &GetDataSourceCapabilitiesRequest,
    jgen: &mut JsonGenerator,
) -> Result<()> {
    crate::serde::v2::metadata_request_serde::serialize_header(request.header(), jgen)
}

pub fn do_typed_deserialize(
    jparser: &mut JsonParser<'_>,
) -> Result<GetDataSourceCapabilitiesRequest> {
    let header = crate::serde::v2::metadata_request_serde::deserialize_header(jparser)?;
    let catalog_name = header.catalog_name()?;
    Ok(GetDataSourceCapabilitiesRequest::new(
        header.identity,
        header.query_id,
        catalog_name,
    ))
}
