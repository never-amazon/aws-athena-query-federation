use crate::exceptions::Result;
use crate::metadata::MetadataRequest;
use crate::request::FederatedIdentity;
use crate::serde::base::{assert_field_name, get_next_string_field};
use crate::serde::federated_identity_serde;
use crate::serde::json::{JsonGenerator, JsonParser};

pub const IDENTITY_FIELD: &str = "identity";
pub const QUERY_ID_FIELD: &str = "queryId";
pub const CATALOG_NAME_FIELD: &str = "catalogName";

pub struct MetadataRequestHeader {
    pub identity: Option<FederatedIdentity>,
    pub query_id: Option<String>,
    pub catalog_name: Option<String>,
}

pub fn serialize_header(request: &dyn MetadataRequest, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_field_name(IDENTITY_FIELD)?;
    federated_identity_serde::serialize(request.identity(), jgen)?;
    jgen.write_string_field(QUERY_ID_FIELD, request.query_id())?;
    jgen.write_string_field(CATALOG_NAME_FIELD, Some(request.catalog_name()))
}

pub fn deserialize_header(jparser: &mut JsonParser<'_>) -> Result<MetadataRequestHeader> {
    assert_field_name(jparser, Some(IDENTITY_FIELD))?;
    let identity = federated_identity_serde::deserialize(jparser)?;
    let query_id = get_next_string_field(jparser, QUERY_ID_FIELD)?;
    let catalog_name = get_next_string_field(jparser, CATALOG_NAME_FIELD)?;
    Ok(MetadataRequestHeader {
        identity,
        query_id,
        catalog_name,
    })
}

impl MetadataRequestHeader {
    pub fn catalog_name(&self) -> Result<String> {
        crate::serde::base::require_non_null(self.catalog_name.clone(), "catalogName is null")
    }
}
