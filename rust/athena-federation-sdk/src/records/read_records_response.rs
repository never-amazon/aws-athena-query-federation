use std::fmt;

use arrow::datatypes::SchemaRef;

use super::{RecordRequestType, RecordResponse};
use crate::data::Block;

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

impl RecordResponse for ReadRecordsResponse {
    fn request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    fn catalog_name(&self) -> &str {
        &self.catalog_name
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
