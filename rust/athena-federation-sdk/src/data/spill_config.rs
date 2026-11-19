use crate::domain::spill::SpillLocation;
use crate::security::EncryptionKey;

const DEFAULT_SPILL_THREADS: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpillConfig {
    pub spill_location: SpillLocation,
    pub request_id: String,
    pub encryption_key: Option<EncryptionKey>,
    pub max_block_bytes: i64,
    pub max_inline_block_bytes: i64,
    pub num_spill_threads: i32,
}

impl SpillConfig {
    pub fn new(spill_location: SpillLocation, request_id: impl Into<String>) -> SpillConfig {
        SpillConfig {
            spill_location,
            request_id: request_id.into(),
            encryption_key: None,
            max_block_bytes: 0,
            max_inline_block_bytes: 0,
            num_spill_threads: DEFAULT_SPILL_THREADS,
        }
    }
}
