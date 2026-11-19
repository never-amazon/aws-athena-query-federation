use std::fmt;

use crate::lambda::LambdaContext;
use crate::metadata::{
    collections_equals, impl_metadata_request, impl_metadata_response, opt_str, MetadataRequestType,
};
use crate::security::FederatedIdentity;

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

#[derive(Debug, Clone)]
pub struct ListSchemasResponse {
    catalog_name: String,
    schemas: Vec<String>,
}

impl ListSchemasResponse {
    pub fn new(catalog_name: impl Into<String>, schemas: Vec<String>) -> ListSchemasResponse {
        ListSchemasResponse {
            catalog_name: catalog_name.into(),
            schemas,
        }
    }

    pub fn schemas(&self) -> &[String] {
        &self.schemas
    }
}

impl_metadata_response!(ListSchemasResponse, MetadataRequestType::ListSchemas);

impl PartialEq for ListSchemasResponse {
    fn eq(&self, other: &Self) -> bool {
        collections_equals(&self.schemas, &other.schemas) && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for ListSchemasResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ListSchemasResponse{{schemas=[{}]}}",
            self.schemas.join(", ")
        )
    }
}
