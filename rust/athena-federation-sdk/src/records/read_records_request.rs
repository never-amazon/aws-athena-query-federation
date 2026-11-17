use std::fmt;

use arrow::datatypes::Schema;

use super::{RecordRequest, RecordRequestType};
use crate::domain::predicate::Constraints;
use crate::domain::split::Split;
use crate::domain::table_name::TableName;
use crate::request::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct ReadRecordsRequest {
    identity: Option<FederatedIdentity>,
    catalog_name: String,
    query_id: String,
    table_name: TableName,
    schema: Schema,
    split: Split,
    constraints: Constraints,
    max_block_size: i64,
    max_inline_block_size: i64,
}

impl ReadRecordsRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        identity: Option<FederatedIdentity>,
        catalog_name: impl Into<String>,
        query_id: impl Into<String>,
        table_name: TableName,
        schema: Schema,
        split: Split,
        constraints: Constraints,
        max_block_size: i64,
        max_inline_block_size: i64,
    ) -> ReadRecordsRequest {
        ReadRecordsRequest {
            identity,
            catalog_name: catalog_name.into(),
            query_id: query_id.into(),
            table_name,
            schema,
            split,
            constraints,
            max_block_size,
            max_inline_block_size,
        }
    }

    pub fn get_identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    pub fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn get_query_id(&self) -> &str {
        &self.query_id
    }

    pub fn get_table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn get_schema(&self) -> &Schema {
        &self.schema
    }

    pub fn get_split(&self) -> &Split {
        &self.split
    }

    pub fn get_max_inline_block_size(&self) -> i64 {
        self.max_inline_block_size
    }

    pub fn get_max_block_size(&self) -> i64 {
        self.max_block_size
    }

    pub fn get_constraints(&self) -> &Constraints {
        &self.constraints
    }
}

impl RecordRequest for ReadRecordsRequest {
    fn get_identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    fn get_query_id(&self) -> &str {
        &self.query_id
    }
}

impl PartialEq for ReadRecordsRequest {
    fn eq(&self, other: &Self) -> bool {
        self.table_name == other.table_name
            && self.schema == other.schema
            && self.split == other.split
            && self.constraints == other.constraints
            && self.max_block_size == other.max_block_size
            && self.max_inline_block_size == other.max_inline_block_size
            && self.catalog_name == other.catalog_name
            && self.query_id == other.query_id
    }
}

impl fmt::Display for ReadRecordsRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ReadRecordsRequest{{queryId={}, tableName={}, schema={}, split={}, requestType={}, catalogName={}, maxBlockSize={}, maxInlineBlockSize={}, constraints={}}}",
            self.query_id,
            self.table_name,
            self.schema,
            self.split,
            self.get_request_type(),
            self.catalog_name,
            self.max_block_size,
            self.max_inline_block_size,
            self.constraints
        )
    }
}
