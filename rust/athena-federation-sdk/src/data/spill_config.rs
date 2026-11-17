use crate::domain::spill::SpillLocation;
use crate::exceptions::{FederationError, Result};
use crate::security::EncryptionKey;

const DEFAULT_SPILL_THREADS: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpillConfig {
    encryption_key: Option<EncryptionKey>,
    spill_location: SpillLocation,
    request_id: String,
    max_block_bytes: i64,
    max_inline_block_size: i64,
    num_spill_threads: i32,
}

impl SpillConfig {
    pub fn get_encryption_key(&self) -> Option<&EncryptionKey> {
        self.encryption_key.as_ref()
    }

    pub fn get_spill_location(&self) -> &SpillLocation {
        &self.spill_location
    }

    pub fn get_request_id(&self) -> &str {
        &self.request_id
    }

    pub fn get_max_block_bytes(&self) -> i64 {
        self.max_block_bytes
    }

    pub fn get_max_inline_block_size(&self) -> i64 {
        self.max_inline_block_size
    }

    pub fn get_num_spill_threads(&self) -> i32 {
        self.num_spill_threads
    }

    pub fn new_builder() -> SpillConfigBuilder {
        SpillConfigBuilder::default()
    }

    pub fn new_builder_from(copy: &SpillConfig) -> SpillConfigBuilder {
        SpillConfigBuilder {
            encryption_key: copy.encryption_key.clone(),
            max_block_bytes: copy.max_block_bytes,
            ..SpillConfigBuilder::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpillConfigBuilder {
    encryption_key: Option<EncryptionKey>,
    request_id: Option<String>,
    spill_location: Option<SpillLocation>,
    max_block_bytes: i64,
    max_inline_block_size: i64,
    num_spill_threads: i32,
}

impl Default for SpillConfigBuilder {
    fn default() -> Self {
        SpillConfigBuilder {
            encryption_key: None,
            request_id: None,
            spill_location: None,
            max_block_bytes: 0,
            max_inline_block_size: 0,
            num_spill_threads: DEFAULT_SPILL_THREADS,
        }
    }
}

impl SpillConfigBuilder {
    #[must_use]
    pub fn with_encryption_key(mut self, val: Option<EncryptionKey>) -> Self {
        self.encryption_key = val;
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, val: impl Into<String>) -> Self {
        self.request_id = Some(val.into());
        self
    }

    #[must_use]
    pub fn with_spill_location(mut self, val: SpillLocation) -> Self {
        self.spill_location = Some(val);
        self
    }

    #[must_use]
    pub fn with_num_spill_threads(mut self, val: i32) -> Self {
        self.num_spill_threads = val;
        self
    }

    #[must_use]
    pub fn with_max_block_bytes(mut self, val: i64) -> Self {
        self.max_block_bytes = val;
        self
    }

    #[must_use]
    pub fn with_max_inline_block_bytes(mut self, val: i64) -> Self {
        self.max_inline_block_size = val;
        self
    }

    pub fn build(self) -> Result<SpillConfig> {
        Ok(SpillConfig {
            encryption_key: self.encryption_key,
            spill_location: self
                .spill_location
                .ok_or_else(|| FederationError::NullPointer("spillLocation was null".into()))?,
            request_id: self
                .request_id
                .ok_or_else(|| FederationError::NullPointer("requestId was null".into()))?,
            max_block_bytes: self.max_block_bytes,
            max_inline_block_size: self.max_inline_block_size,
            num_spill_threads: self.num_spill_threads,
        })
    }
}
