use std::fmt;

use super::metadata_request::impl_metadata_request;
use super::{opt_str, MetadataRequestType};
use crate::request::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct GetDataSourceCapabilitiesRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
}

impl GetDataSourceCapabilitiesRequest {
    pub fn new(identity: Option<FederatedIdentity>, query_id: Option<String>, catalog_name: impl Into<String>) -> GetDataSourceCapabilitiesRequest {
        GetDataSourceCapabilitiesRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
        }
    }
}

impl_metadata_request!(GetDataSourceCapabilitiesRequest, MetadataRequestType::GetDatasourceCapabilities);

impl PartialEq for GetDataSourceCapabilitiesRequest {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity && self.query_id == other.query_id && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetDataSourceCapabilitiesRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GetDataSourceCapabilitiesRequest{{identity={}, queryId={}, catalogName={}}}",
            self.identity.as_ref().map(|i| i.to_string()).unwrap_or_else(|| "null".into()),
            opt_str(self.query_id.as_deref()),
            self.catalog_name
        )
    }
}
