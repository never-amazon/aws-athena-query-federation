use std::fmt;

use arrow::datatypes::Schema;

use super::{RecordRequestType, RecordResponse};
use crate::domain::spill::SpillLocation;
use crate::security::EncryptionKey;

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

    pub fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    pub fn get_catalog_name(&self) -> &str {
        &self.catalog_name
    }

    pub fn get_schema(&self) -> &Schema {
        &self.schema
    }

    pub fn get_remote_blocks(&self) -> &[SpillLocation] {
        &self.remote_blocks
    }

    pub fn get_number_blocks(&self) -> usize {
        self.remote_blocks.len()
    }

    pub fn get_encryption_key(&self) -> Option<&EncryptionKey> {
        self.encryption_key.as_ref()
    }
}

impl RecordResponse for RemoteReadRecordsResponse {
    fn get_request_type(&self) -> RecordRequestType {
        RecordRequestType::ReadRecords
    }

    fn get_catalog_name(&self) -> &str {
        &self.catalog_name
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
        let blocks: Vec<String> = self.remote_blocks.iter().map(|b| b.to_string()).collect();
        write!(
            f,
            "RemoteReadRecordsResponse{{schema={}, remoteBlocks=[{}], encryptionKey=XXXXXX, requestType={}, catalogName={}}}",
            self.schema,
            blocks.join(", "),
            self.get_request_type(),
            self.catalog_name
        )
    }
}
