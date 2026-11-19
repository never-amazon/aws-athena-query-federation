use std::fmt;

use arrow::datatypes::{Schema, SchemaRef};

use crate::data::Block;
use crate::domain::predicate::Constraints;
use crate::domain::spill::SpillLocation;
use crate::domain::split::Split;
use crate::domain::table_name::TableName;
use crate::named_enum::named_enum;
use crate::security::{EncryptionKey, FederatedIdentity};

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
    #[allow(clippy::too_many_arguments, reason = "mirrors the Java constructor")]
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

    pub fn identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    pub fn request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn query_id(&self) -> &str {
        &self.query_id
    }

    pub fn table_name(&self) -> &TableName {
        &self.table_name
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn split(&self) -> &Split {
        &self.split
    }

    pub fn max_inline_block_size(&self) -> i64 {
        self.max_inline_block_size
    }

    pub fn max_block_size(&self) -> i64 {
        self.max_block_size
    }

    pub fn constraints(&self) -> &Constraints {
        &self.constraints
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
            self.request_type(),
            self.catalog_name,
            self.max_block_size,
            self.max_inline_block_size,
            self.constraints
        )
    }
}

named_enum! {
    pub enum RecordRequestType {
        ReadRecords => "READ_RECORDS",
    }
}

#[derive(Debug, Clone)]
pub struct ReadRecordsResponse {
    catalog_name: String,
    records: Block,
}

impl ReadRecordsResponse {
    pub fn new(catalog_name: impl Into<String>, records: Block) -> ReadRecordsResponse {
        ReadRecordsResponse {
            catalog_name: catalog_name.into(),
            records,
        }
    }

    pub fn request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn schema(&self) -> &SchemaRef {
        self.records.schema()
    }

    pub fn records(&self) -> &Block {
        &self.records
    }

    pub fn record_count(&self) -> usize {
        self.records.row_count()
    }
}

impl PartialEq for ReadRecordsResponse {
    fn eq(&self, other: &Self) -> bool {
        self.records == other.records && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for ReadRecordsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ReadRecordsResponse{{records={}, requestType={}, catalogName={}}}",
            self.records,
            self.request_type(),
            self.catalog_name
        )
    }
}

#[derive(Debug, Clone)]
pub struct RemoteReadRecordsResponse {
    catalog_name: String,
    schema: Schema,
    remote_blocks: Vec<SpillLocation>,
    encryption_key: Option<EncryptionKey>,
}

impl RemoteReadRecordsResponse {
    pub fn new(
        catalog_name: impl Into<String>,
        schema: Schema,
        remote_blocks: Vec<SpillLocation>,
        encryption_key: Option<EncryptionKey>,
    ) -> RemoteReadRecordsResponse {
        RemoteReadRecordsResponse {
            catalog_name: catalog_name.into(),
            schema,
            remote_blocks,
            encryption_key,
        }
    }

    pub fn request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn remote_blocks(&self) -> &[SpillLocation] {
        &self.remote_blocks
    }

    pub fn number_blocks(&self) -> usize {
        self.remote_blocks.len()
    }

    pub fn encryption_key(&self) -> Option<&EncryptionKey> {
        self.encryption_key.as_ref()
    }
}

impl PartialEq for RemoteReadRecordsResponse {
    fn eq(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.remote_blocks == other.remote_blocks
            && self.encryption_key == other.encryption_key
            && self.catalog_name == other.catalog_name
    }
}

impl fmt::Display for RemoteReadRecordsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let blocks: Vec<String> = self
            .remote_blocks
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        write!(
            f,
            "RemoteReadRecordsResponse{{schema={}, remoteBlocks=[{}], encryptionKey=XXXXXX, requestType={}, catalogName={}}}",
            self.schema,
            blocks.join(", "),
            self.request_type(),
            self.catalog_name
        )
    }
}
