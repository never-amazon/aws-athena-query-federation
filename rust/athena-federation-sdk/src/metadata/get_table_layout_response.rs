use std::fmt;

use super::metadata_response::impl_metadata_response;
use super::MetadataRequestType;
use crate::data::Block;
use crate::domain::table_name::TableName;

#[derive(Debug, Clone)]
pub struct GetTableLayoutResponse {
    catalog_name: String,
    table_name: TableName,
    partitions: Block,
}

impl GetTableLayoutResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        table_name: TableName,
        partitions: Block,
    ) -> GetTableLayoutResponse {
        GetTableLayoutResponse {
            catalog_name: catalog_name.into(),
            table_name,
            partitions,
        }
    }

    pub fn get_table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn get_partitions(&self) -> &Block {
        &self.partitions
    }
}

impl_metadata_response!(GetTableLayoutResponse, MetadataRequestType::GetTableLayout);

impl PartialEq for GetTableLayoutResponse {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.partitions == other.partitions
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetTableLayoutResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GetTableLayoutResponse{{tableName={}, requestType={}, catalogName={}}}",
            self.table_name,
            self.get_request_type(),
            self.catalog_name
        )
    }
}
