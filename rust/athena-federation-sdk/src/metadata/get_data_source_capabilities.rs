use std::fmt;

use indexmap::IndexMap;

use crate::lambda::LambdaContext;
use crate::metadata::{
    impl_metadata_request, impl_metadata_response, opt_str, MetadataRequestType,
    OptimizationSubType,
};
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

#[derive(Debug, Clone)]
pub struct GetDataSourceCapabilitiesResponse {
    catalog_name: String,
    capabilities: IndexMap<String, Vec<OptimizationSubType>>,
}

impl GetDataSourceCapabilitiesResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        capabilities: IndexMap<String, Vec<OptimizationSubType>>,
    ) -> GetDataSourceCapabilitiesResponse {
        GetDataSourceCapabilitiesResponse {
            catalog_name: catalog_name.into(),
            capabilities,
        }
    }

    pub fn capabilities(&self) -> &IndexMap<String, Vec<OptimizationSubType>> {
        &self.capabilities
    }
}

impl_metadata_response!(
    GetDataSourceCapabilitiesResponse,
    MetadataRequestType::GetDatasourceCapabilities
);

impl PartialEq for GetDataSourceCapabilitiesResponse {
    fn eq(&self, other: &Self) -> bool {
        self.capabilities == other.capabilities && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetDataSourceCapabilitiesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let caps: Vec<String> = self
            .capabilities
            .iter()
            .map(|(k, v)| {
                let items: Vec<String> = v.iter().map(std::string::ToString::to_string).collect();
                format!("{}=[{}]", k, items.join(", "))
            })
            .collect();
        write!(
            f,
            "GetDataSourceCapabilitiesResponse{{capabilities={{{}}}, requestType={}, catalogName={}}}",
            caps.join(", "),
            self.request_type(),
            self.catalog_name
        )
    }
}
