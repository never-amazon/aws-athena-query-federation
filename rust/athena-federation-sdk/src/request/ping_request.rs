use std::fmt;

use super::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct PingRequest {
    identity: Option<FederatedIdentity>,
    catalog_name: String,
    query_id: String,
}

impl PingRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        catalog_name: impl Into<String>,
        query_id: impl Into<String>,
    ) -> PingRequest {
        PingRequest {
            identity,
            catalog_name: catalog_name.into(),
            query_id: query_id.into(),
        }
    }

    pub fn get_identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    pub fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn get_query_id(&self) -> &str {
        &self.query_id
    }
}

impl PartialEq for PingRequest {
    fn eq(&self, other: &Self) -> bool {
        self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for PingRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PingRequest{{catalogName='{}', queryId='{}'}}",
            self.catalog_name, self.query_id
        )
    }
}
