use std::sync::Arc;

use crate::data::BlockAllocator;
use crate::serde::ObjectMapper;

pub fn create(allocator: Arc<dyn BlockAllocator>) -> ObjectMapper {
    ObjectMapper::bean(allocator)
}
