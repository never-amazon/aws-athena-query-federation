use std::collections::HashMap;
use std::fmt;

use crate::serde::java_hash::java_hash_map_order;

use super::metadata_request::impl_metadata_request;
use super::{opt_str, MetadataRequestType};
use crate::domain::table_name::TableName;
use crate::request::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct GetTableRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    table_name: TableName,
    query_passthrough_arguments: HashMap<String, Option<String>>,
}

impl GetTableRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
        table_name: TableName,
        query_passthrough_arguments: HashMap<String, Option<String>>,
    ) -> GetTableRequest {
        GetTableRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            table_name,
            query_passthrough_arguments,
        }
    }

    pub fn get_table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn get_query_passthrough_arguments(&self) -> &HashMap<String, Option<String>> {
        &self.query_passthrough_arguments
    }

    pub fn is_query_passthrough(&self) -> bool {
        !self.query_passthrough_arguments.is_empty()
    }
}

impl_metadata_request!(GetTableRequest, MetadataRequestType::GetTable);

impl PartialEq for GetTableRequest {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.catalog_name == other.catalog_name
            && self.query_passthrough_arguments == other.query_passthrough_arguments
    }
}

impl fmt::Display for GetTableRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args: Vec<String> = java_hash_map_order(self.query_passthrough_arguments.keys())
            .into_iter()
            .map(|k| {
                format!(
                    "{}={}",
                    k,
                    self.query_passthrough_arguments
                        .get(k)
                        .and_then(|v| v.as_deref())
                        .unwrap_or("null")
                )
            })
            .collect();
        write!(
            f,
            "GetTableRequest{{queryId={}, tableName={}queryPassthroughArguments={{{}}}}}",
            opt_str(self.query_id.as_deref()),
            self.table_name,
            args.join(", ")
        )
    }
}
