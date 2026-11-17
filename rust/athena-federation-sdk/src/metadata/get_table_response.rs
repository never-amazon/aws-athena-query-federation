use std::fmt;

use arrow::datatypes::Schema;
use indexmap::IndexSet;

use super::metadata_response::impl_metadata_response;
use super::MetadataRequestType;
use crate::domain::table_name::TableName;

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
