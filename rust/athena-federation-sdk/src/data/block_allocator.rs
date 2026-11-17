use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::{Schema, SchemaRef};

use super::block::Block;
use super::record_batch_serde::ArrowRecordBatch;
use crate::exceptions::{FederationError, Result};

pub type ArrowBuf = Vec<u8>;

pub type BatchGenerator<'a> = dyn FnMut() -> Result<ArrowRecordBatch> + 'a;

pub trait BlockAllocator: Send + Sync {
    fn create_block(&self, schema: SchemaRef) -> Block;

    fn create_buffer(&self, size: usize) -> Result<ArrowBuf>;

    fn register_batch(&self, generator: &mut BatchGenerator<'_>) -> Result<ArrowRecordBatch>;

    fn usage(&self) -> usize;

    fn close(&self);

    fn is_closed(&self) -> bool;

    fn create_block_from_schema(&self, schema: &Schema) -> Block {
        self.create_block(Arc::new(schema.clone()))
    }
}

pub trait BlockAllocatorRegistry {
    fn get_or_create_allocator(&self, id: &str) -> Arc<dyn BlockAllocator>;
}

#[derive(Debug)]
pub struct BlockAllocatorImpl {
    id: String,
    memory_limit: usize,
    usage: AtomicUsize,
    blocks_created: AtomicUsize,
    record_batches: Mutex<Vec<usize>>,
    is_closed: AtomicBool,
}

impl Default for BlockAllocatorImpl {
    fn default() -> Self {
        BlockAllocatorImpl::new()
    }
}

impl BlockAllocatorImpl {
    pub fn new() -> BlockAllocatorImpl {
        BlockAllocatorImpl::with_id_and_limit(uuid::Uuid::new_v4().to_string(), i32::MAX as usize)
    }

    pub fn with_id(id: impl Into<String>) -> BlockAllocatorImpl {
        BlockAllocatorImpl::with_id_and_limit(id, i32::MAX as usize)
    }

    pub fn with_id_and_limit(id: impl Into<String>, memory_limit: usize) -> BlockAllocatorImpl {
        BlockAllocatorImpl {
            id: id.into(),
            memory_limit,
            usage: AtomicUsize::new(0),
            blocks_created: AtomicUsize::new(0),
            record_batches: Mutex::new(Vec::new()),
            is_closed: AtomicBool::new(false),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn memory_limit(&self) -> usize {
        self.memory_limit
    }

    pub fn blocks_created(&self) -> usize {
        self.blocks_created.load(Ordering::SeqCst)
    }

    fn reserve(&self, bytes: usize) -> Result<()> {
        let current = self.usage.load(Ordering::SeqCst);
        if current.saturating_add(bytes) > self.memory_limit {
            return Err(FederationError::Runtime(format!(
                "Unable to allocate buffer of size {bytes} due to memory limit. Current allocation: {current}"
            )));
        }
        self.usage.fetch_add(bytes, Ordering::SeqCst);
        Ok(())
    }
}

impl BlockAllocator for BlockAllocatorImpl {
    fn create_block(&self, schema: SchemaRef) -> Block {
        self.blocks_created.fetch_add(1, Ordering::SeqCst);
        Block::new(self.id.clone(), schema)
    }

    fn create_buffer(&self, size: usize) -> Result<ArrowBuf> {
        self.reserve(size)?;
        Ok(vec![0u8; size])
    }

    fn register_batch(&self, generator: &mut BatchGenerator<'_>) -> Result<ArrowRecordBatch> {
        let batch = generator()?;
        let size = batch.compute_body_length();
        self.reserve(size)?;
        if let Ok(mut batches) = self.record_batches.lock() {
            batches.push(size);
        }
        Ok(batch)
    }

    fn usage(&self) -> usize {
        self.usage.load(Ordering::SeqCst)
    }

    fn close(&self) {
        if !self.is_closed.swap(true, Ordering::SeqCst) {
            if let Ok(mut batches) = self.record_batches.lock() {
                batches.clear();
            }
            self.usage.store(0, Ordering::SeqCst);
        }
    }

    fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::SeqCst)
    }
}
