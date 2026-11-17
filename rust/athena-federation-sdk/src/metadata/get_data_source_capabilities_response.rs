use std::fmt;

use indexmap::IndexMap;

use super::metadata_response::impl_metadata_response;
use super::optimizations::OptimizationSubType;
use super::MetadataRequestType;

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

    pub fn get_capabilities(&self) -> &IndexMap<String, Vec<OptimizationSubType>> {
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
            self.get_request_type(),
            self.catalog_name
        )
    }
}
