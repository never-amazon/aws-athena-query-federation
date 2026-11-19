use std::collections::HashMap;
use std::fmt;

use arrow::datatypes::Schema;
use indexmap::IndexSet;

use crate::domain::table_name::TableName;
use crate::lambda::LambdaContext;
use crate::metadata::{
    impl_metadata_request, impl_metadata_response, opt_str, MetadataRequestType,
};
use crate::security::FederatedIdentity;
use crate::serde::java_hash::java_hash_map_order;

#[derive(Debug, Clone)]
pub struct GetTableRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    table_name: TableName,
    query_passthrough_arguments: HashMap<String, Option<String>>,
    context: Option<LambdaContext>,
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
            context: None,
        }
    }

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn query_passthrough_arguments(&self) -> &HashMap<String, Option<String>> {
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

#[derive(Debug, Clone)]
pub struct GetTableResponse {
    catalog_name: String,
    table_name: TableName,
    schema: Schema,
    partition_columns: IndexSet<String>,
}

impl GetTableResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        table_name: TableName,
        schema: Schema,
        partition_columns: IndexSet<String>,
    ) -> GetTableResponse {
        GetTableResponse {
            catalog_name: catalog_name.into(),
            table_name,
            schema,
            partition_columns,
        }
    }

    pub fn without_partitions(
        catalog_name: impl Into<String>,
        table_name: TableName,
        schema: Schema,
    ) -> GetTableResponse {
        GetTableResponse::new(catalog_name, table_name, schema, IndexSet::new())
    }

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn partition_columns(&self) -> &IndexSet<String> {
        &self.partition_columns
    }
}

impl_metadata_response!(GetTableResponse, MetadataRequestType::GetTable);

impl PartialEq for GetTableResponse {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.schema == other.schema
            && self.partition_columns == other.partition_columns
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetTableResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cols: Vec<&str> = self
            .partition_columns
            .iter()
            .map(std::string::String::as_str)
            .collect();
        write!(
            f,
            "GetTableResponse{{tableName={}, schema={}, partitionColumns=[{}], requestType={}, catalogName={}}}",
            self.table_name,
            self.schema,
            cols.join(", "),
            self.request_type(),
            self.catalog_name
        )
    }
}
