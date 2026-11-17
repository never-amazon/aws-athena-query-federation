use std::fmt;

use super::metadata_response::impl_metadata_response;
use super::{collections_equals, opt_str, MetadataRequestType};
use crate::domain::table_name::TableName;

#[derive(Debug, Clone)]
pub struct ListTablesResponse {
    catalog_name: String,
    tables: Vec<TableName>,
    next_token: Option<String>,
}

impl ListTablesResponse {
    pub fn new(catalog_name: impl Into<String>, tables: Vec<TableName>, next_token: Option<String>) -> ListTablesResponse {
        ListTablesResponse {
            catalog_name: catalog_name.into(),
            tables,
            next_token,
        }
    }

    pub fn get_tables(&self) -> &[TableName] {
        &self.tables
    }

    pub fn get_next_token(&self) -> Option<&str> {
        self.next_token.as_deref()
    }
}

impl_metadata_response!(ListTablesResponse, MetadataRequestType::ListTables);

impl PartialEq for ListTablesResponse {
    fn eq(&self, other: &Self) -> bool {
        collections_equals(&self.tables, &other.tables)
            && self.catalog_name == other.catalog_name
            && self.next_token == other.next_token
    }
}

impl fmt::Display for ListTablesResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tables: Vec<String> = self.tables.iter().map(|t| t.to_string()).collect();
        write!(
            f,
            "ListTablesResponse{{tables=[{}], nextToken='{}'}}",
            tables.join(", "),
            opt_str(self.next_token.as_deref())
        )
    }
}
