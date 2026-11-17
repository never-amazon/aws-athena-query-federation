use std::fmt;

use super::metadata_response::impl_metadata_response;
use super::{collections_equals, MetadataRequestType};

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

    pub fn get_schemas(&self) -> &[String] {
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
        write!(f, "ListSchemasResponse{{schemas=[{}]}}", self.schemas.join(", "))
    }
}
