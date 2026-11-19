use std::fmt;

use arrow::datatypes::Schema;
use indexmap::IndexSet;

use crate::data::Block;
use crate::domain::predicate::Constraints;
use crate::domain::TableName;
use crate::lambda::LambdaContext;
use crate::metadata::{
    impl_metadata_request, impl_metadata_response, opt_str, MetadataRequestType,
};
use crate::security::FederatedIdentity;
use crate::serde::java_hash::java_hash_set_copy_order;

#[derive(Debug, Clone)]
pub struct GetTableLayoutRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    table_name: TableName,
    constraints: Constraints,
    schema: Schema,
    partition_cols: IndexSet<String>,
    context: Option<LambdaContext>,
}

impl GetTableLayoutRequest {
    pub fn new<I, S>(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
        table_name: TableName,
        constraints: Constraints,
        schema: Schema,
        partition_cols: I,
    ) -> GetTableLayoutRequest
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let given: Vec<String> = partition_cols
            .into_iter()
            .map(std::convert::Into::into)
            .collect();
        let partition_cols: IndexSet<String> =
            java_hash_set_copy_order(given.iter().map(std::string::String::as_str))
                .into_iter()
                .map(std::string::ToString::to_string)
                .collect();
        GetTableLayoutRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            table_name,
            constraints,
            schema,
            partition_cols,
            context: None,
        }
    }

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn constraints(&self) -> &Constraints {
        &self.constraints
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn partition_cols(&self) -> &IndexSet<String> {
        &self.partition_cols
    }
}

impl_metadata_request!(GetTableLayoutRequest, MetadataRequestType::GetTableLayout);

impl PartialEq for GetTableLayoutRequest {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.constraints == other.constraints
            && self.schema == other.schema
            && self.partition_cols == other.partition_cols
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for GetTableLayoutRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cols: Vec<&str> = self
            .partition_cols
            .iter()
            .map(std::string::String::as_str)
            .collect();
        write!(
            f,
            "GetTableLayoutRequest{{queryId={}, tableName={}, constraints={}, schema={}, partitionCols=[{}]}}",
            opt_str(self.query_id.as_deref()),
            self.table_name,
            self.constraints,
            self.schema,
            cols.join(", ")
        )
    }
}

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

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn partitions(&self) -> &Block {
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
            self.request_type(),
            self.catalog_name
        )
    }
}
