use std::fmt;

use super::metadata_request::impl_metadata_request;
use super::{opt_str, MetadataRequestType};
use crate::lambda::LambdaContext;
use crate::security::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct GetDataSourceCapabilitiesRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    context: Option<LambdaContext>,
}

impl GetDataSourceCapabilitiesRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
    ) -> GetDataSourceCapabilitiesRequest {
        GetDataSourceCapabilitiesRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            context: None,
        }
    }
}

impl_metadata_request!(
    GetDataSourceCapabilitiesRequest,
    MetadataRequestType::GetDatasourceCapabilities
);

impl PartialEq for GetDataSourceCapabilitiesRequest {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.query_id == other.query_id
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetDataSourceCapabilitiesRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GetDataSourceCapabilitiesRequest{{identity={}, queryId={}, catalogName={}}}",
            self.identity
                .as_ref()
                .map_or_else(|| "null".into(), std::string::ToString::to_string),
            opt_str(self.query_id.as_deref()),
            self.catalog_name
        )
    }
}
