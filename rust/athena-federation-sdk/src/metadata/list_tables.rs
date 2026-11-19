use std::fmt;

use crate::domain::table_name::TableName;
use crate::lambda::LambdaContext;
use crate::metadata::{
    collections_equals, impl_metadata_request, impl_metadata_response, opt_str, MetadataRequestType,
};
use crate::security::FederatedIdentity;

pub const UNLIMITED_PAGE_SIZE_VALUE: i32 = -1;

#[derive(Debug, Clone)]
pub struct ListTablesRequest {
    identity: Option<FederatedIdentity>,
    query_id: Option<String>,
    catalog_name: String,
    schema_name: Option<String>,
    next_token: Option<String>,
    page_size: i32,
    context: Option<LambdaContext>,
}

impl ListTablesRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        query_id: Option<String>,
        catalog_name: impl Into<String>,
        schema_name: Option<String>,
        next_token: Option<String>,
        page_size: i32,
    ) -> ListTablesRequest {
        ListTablesRequest {
            identity,
            query_id,
            catalog_name: catalog_name.into(),
            schema_name,
            next_token,
            page_size,
            context: None,
        }
    }

    pub fn schema_name(&self) -> Option<&str> {
        self.schema_name.as_deref()
    }

    pub fn next_token(&self) -> Option<&str> {
        self.next_token.as_deref()
    }

    pub fn page_size(&self) -> i32 {
        self.page_size
    }
}

impl_metadata_request!(ListTablesRequest, MetadataRequestType::ListTables);

impl PartialEq for ListTablesRequest {
    fn eq(&self, other: &Self) -> bool {
        self.schema_name == other.schema_name
            && self.catalog_name == other.catalog_name
            && self.next_token == other.next_token
            && self.page_size == other.page_size
    }
}

impl fmt::Display for ListTablesRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ListTablesRequest{{queryId={}, schemaName='{}', nextToken='{}', pageSize={}}}",
            opt_str(self.query_id.as_deref()),
            opt_str(self.schema_name.as_deref()),
            opt_str(self.next_token.as_deref()),
            self.page_size
        )
    }
}

#[derive(Debug, Clone)]
pub struct ListTablesResponse {
    catalog_name: String,
    tables: Vec<TableName>,
    next_token: Option<String>,
}

impl ListTablesResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        tables: Vec<TableName>,
        next_token: Option<String>,
    ) -> ListTablesResponse {
        ListTablesResponse {
            catalog_name: catalog_name.into(),
            tables,
            next_token,
        }
    }

    pub fn tables(&self) -> &[TableName] {
        &self.tables
    }

    pub fn next_token(&self) -> Option<&str> {
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
        let tables: Vec<String> = self
            .tables
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        write!(
            f,
            "ListTablesResponse{{tables=[{}], nextToken='{}'}}",
            tables.join(", "),
            opt_str(self.next_token.as_deref())
        )
    }
}
