use std::fmt;

use super::metadata_request::impl_metadata_request;
use super::{opt_str, MetadataRequestType};
use crate::lambda::LambdaContext;
use crate::request::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct ListSchemasRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    context: Option<LambdaContext>,
}

impl ListSchemasRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
    ) -> ListSchemasRequest {
        ListSchemasRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            context: None,
        }
    }
}

impl_metadata_request!(ListSchemasRequest, MetadataRequestType::ListSchemas);

impl PartialEq for ListSchemasRequest {
    fn eq(&self, other: &Self) -> bool {
        self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for ListSchemasRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ListSchemasRequest{{queryId={}}}",
            opt_str(self.query_id.as_deref())
        )
    }
}
