use std::sync::Arc;

use crate::data::BlockAllocator;
use crate::serde::{ObjectMapper, SERDE_VERSION_TWO};

pub fn create(allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
    ObjectMapper::strict(SERDE_VERSION_TWO, allocator)
}
