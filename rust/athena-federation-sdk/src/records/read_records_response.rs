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

    pub fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn get_schema(&self) -> &SchemaRef {
        self.records.get_schema()
    }

    pub fn get_records(&self) -> &Block {
        &self.records
    }

    pub fn get_record_count(&self) -> usize {
        self.records.get_row_count()
    }
}

impl RecordResponse for ReadRecordsResponse {
    fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    fn get_catalog_name(&self) -> &str {
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
            self.get_request_type(),
            self.catalog_name
        )
    }
}
