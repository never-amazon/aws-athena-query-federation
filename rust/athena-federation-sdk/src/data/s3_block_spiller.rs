use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use arrow::datatypes::SchemaRef;
use async_trait::async_trait;
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinHandle;

use super::block::Block;
use super::block_allocator::BlockAllocator;
use super::block_spiller::BlockSpiller;
use super::block_writer::RowWriter;
use super::s3_block_spill_reader::require_present;
use super::s3_client::{GetObjectRequest, PutObjectRequest, S3Client};
use super::spill_config::SpillConfig;
use crate::domain::predicate::ConstraintEvaluator;
use crate::domain::spill::{S3SpillLocation, SpillLocation};
use crate::exceptions::{FederationError, Result};
use crate::java_cast::len_to_long;
use crate::security::{
    run_blocking, AesGcmBlockCrypto, AwsRequestOverrideConfiguration, BlockCrypto, EncryptionKey,
    NoOpBlockCrypto,
};

const ASYNC_SHUTDOWN_MILLIS: u64 = 10_000;
const MAX_ROWS_PER_CALL: i32 = 100;
const SPILL_QUEUE_CAPACITY: &str = "SPILL_QUEUE_CAPACITY";
const SPILL_PUT_REQUEST_HEADERS_ENV: &str = "spill_put_request_headers";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct SpillPool {
    slots: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    shutdown: std::sync::atomic::AtomicBool,
}

struct SpillWriter {
    amazon_s3: Arc<dyn S3Client>,
    block_crypto: Arc<dyn BlockCrypto>,
    spill_config: SpillConfig,
    config_options: HashMap<String, String>,
    spill_number: AtomicI64,
    total_bytes_spilled: AtomicI64,
    async_exception: Mutex<Option<FederationError>>,
    spill_locations: Mutex<Vec<SpillLocation>>,
}

impl SpillWriter {
    fn get_request_headers_from_env(&self) -> Vec<(String, String)> {
        let Some(headers_from_env_str) = self
            .config_options
            .get(SPILL_PUT_REQUEST_HEADERS_ENV)
            .filter(|s| !s.is_empty())
        else {
            return Vec::new();
        };
        match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
            headers_from_env_str,
        ) {
            Ok(map) => {
                let mut headers = Vec::new();
                for (name, value) in map {
                    match value {
                        serde_json::Value::String(s) => headers.push((name, s)),
                        serde_json::Value::Null => {}
                        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                            tracing::error!(
                                "Invalid value for environment variable: {} : {}",
                                SPILL_PUT_REQUEST_HEADERS_ENV,
                                headers_from_env_str
                            );
                            return Vec::new();
                        }
                        other => headers.push((name, other.to_string())),
                    }
                }
                headers
            }
            Err(e) => {
                tracing::error!(
                    "Invalid value for environment variable: {} : {}: {}",
                    SPILL_PUT_REQUEST_HEADERS_ENV,
                    headers_from_env_str,
                    e
                );
                Vec::new()
            }
        }
    }

    fn create_request_override_config(&self) -> Option<AwsRequestOverrideConfiguration> {
        let headers = self.get_request_headers_from_env();
        if headers.is_empty() {
            return None;
        }
        let mut builder = AwsRequestOverrideConfiguration::builder();
        for (name, value) in headers {
            builder = builder.put_header(name, value);
        }
        Some(builder.build())
    }

    fn make_spill_location(&self) -> Result<S3SpillLocation> {
        let SpillLocation::S3(split_spill_location) = self.spill_config.spill_location();
        if !split_spill_location.is_directory() {
            return Err(FederationError::internal(
                "Split's SpillLocation must be a directory because multiple blocks may be spilled.",
            ));
        }
        let block_key = format!(
            "{}.{}",
            split_spill_location.key(),
            self.spill_number.fetch_add(1, Ordering::SeqCst)
        );
        Ok(S3SpillLocation::new(
            split_spill_location.bucket(),
            block_key,
            false,
        ))
    }

    async fn write_unrecorded(
        &self,
        spill_location: Result<S3SpillLocation>,
        block: Block,
    ) -> Result<SpillLocation> {
        let spill_location = spill_location?;
        let encryption_key = self.spill_config.encryption_key().cloned();
        tracing::info!(
            "write: Started encrypting block for write to {}",
            spill_location
        );
        let block_crypto = Arc::clone(&self.block_crypto);
        let bytes =
            run_blocking(move || block_crypto.encrypt(encryption_key.as_ref(), &block)).await?;
        self.total_bytes_spilled
            .fetch_add(len_to_long(bytes.len()), Ordering::SeqCst);
        tracing::info!(
            "write: Started spilling block of size {} bytes",
            bytes.len()
        );
        let mut request = PutObjectRequest::builder()
            .bucket(spill_location.bucket())
            .key(spill_location.key())
            .content_length(len_to_long(bytes.len()));
        if let Some(override_configuration) = self.create_request_override_config() {
            request = request.override_configuration(override_configuration);
        }
        let size = bytes.len();
        self.amazon_s3.put_object(request.build(), bytes).await?;
        tracing::info!("write: Completed spilling block of size {} bytes", size);
        Ok(SpillLocation::S3(spill_location))
    }

    async fn write(&self, block: Block) -> Result<SpillLocation> {
        self.write_to(self.make_spill_location(), block).await
    }

    async fn write_to(
        &self,
        spill_location: Result<S3SpillLocation>,
        block: Block,
    ) -> Result<SpillLocation> {
        match self.write_unrecorded(spill_location, block).await {
            Ok(location) => Ok(location),
            Err(ex) => {
                let mut async_exception = lock(&self.async_exception);
                if async_exception.is_none() {
                    *async_exception = Some(ex.clone());
                }
                tracing::warn!("write: Encountered error while writing block. {}", ex);
                Err(ex)
            }
        }
    }
}

pub struct S3BlockSpiller {
    writer: Arc<SpillWriter>,
    allocator: Arc<dyn BlockAllocator>,
    schema: SchemaRef,
    max_rows_per_call: i64,
    in_progress_block: Mutex<Option<Block>>,
    async_spill_pool: Option<SpillPool>,
    spill_lock: Arc<RwLock<()>>,
    constraint_evaluator: Arc<ConstraintEvaluator>,
    start_time: Instant,
}

impl S3BlockSpiller {
    pub fn new(
        amazon_s3: Arc<dyn S3Client>,
        spill_config: SpillConfig,
        allocator: Arc<dyn BlockAllocator>,
        schema: SchemaRef,
        constraint_evaluator: Arc<ConstraintEvaluator>,
        config_options: HashMap<String, String>,
    ) -> Result<S3BlockSpiller> {
        S3BlockSpiller::new_with_max_rows_per_call(
            amazon_s3,
            spill_config,
            allocator,
            schema,
            constraint_evaluator,
            MAX_ROWS_PER_CALL,
            config_options,
        )
    }

    pub fn new_with_max_rows_per_call(
        amazon_s3: Arc<dyn S3Client>,
        spill_config: SpillConfig,
        allocator: Arc<dyn BlockAllocator>,
        schema: SchemaRef,
        constraint_evaluator: Arc<ConstraintEvaluator>,
        max_rows_per_call: i32,
        config_options: HashMap<String, String>,
    ) -> Result<S3BlockSpiller> {
        let block_crypto: Arc<dyn BlockCrypto> = if spill_config.encryption_key().is_some() {
            Arc::new(AesGcmBlockCrypto::new(allocator.clone()))
        } else {
            Arc::new(NoOpBlockCrypto::new(allocator.clone()))
        };
        let async_spill_pool = if spill_config.num_spill_threads() <= 0 {
            None
        } else {
            Some(Self::make_async_spill_pool(&spill_config, &config_options)?)
        };
        Ok(S3BlockSpiller {
            writer: Arc::new(SpillWriter {
                amazon_s3,
                block_crypto,
                spill_config,
                config_options,
                spill_number: AtomicI64::new(0),
                total_bytes_spilled: AtomicI64::new(0),
                async_exception: Mutex::new(None),
                spill_locations: Mutex::new(Vec::new()),
            }),
            allocator,
            schema,
            max_rows_per_call: i64::from(max_rows_per_call),
            in_progress_block: Mutex::new(None),
            async_spill_pool,
            spill_lock: Arc::new(RwLock::new(())),
            constraint_evaluator,
            start_time: Instant::now(),
        })
    }

    pub fn max_rows_per_call(&self) -> i64 {
        self.max_rows_per_call
    }

    fn make_async_spill_pool(
        config: &SpillConfig,
        config_options: &HashMap<String, String>,
    ) -> Result<SpillPool> {
        let threads = usize::try_from(config.num_spill_threads()).unwrap_or_default();
        let mut spill_queue_capacity = threads;
        let capacity = config_options
            .get(SPILL_QUEUE_CAPACITY)
            .filter(|c| !c.trim().is_empty())
            .or_else(|| config_options.get(&SPILL_QUEUE_CAPACITY.to_lowercase()));
        if let Some(capacity) = capacity {
            let parsed = capacity.parse::<i32>().map_err(|_| {
                FederationError::NumberFormat(format!("For input string: \"{capacity}\""))
            })?;
            if parsed <= 0 {
                return Err(FederationError::IllegalArgument(format!(
                    "Illegal queue capacity {parsed}"
                )));
            }
            spill_queue_capacity = usize::try_from(parsed).unwrap_or_default();
            tracing::debug!("Setting Spill Queue Capacity to {}", spill_queue_capacity);
        }
        Ok(SpillPool {
            slots: Arc::new(Semaphore::new(threads + spill_queue_capacity)),
            workers: Arc::new(Semaphore::new(threads)),
            tasks: Mutex::new(Vec::new()),
            shutdown: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn new_block(&self) -> Block {
        let mut block = self.allocator.create_block(self.schema.clone());
        block.constrain(Some(self.constraint_evaluator.clone()));
        block
    }

    fn ensure_init<'a>(&self, slot: &'a mut Option<Block>) -> &'a mut Block {
        slot.get_or_insert_with(|| self.new_block())
    }

    pub async fn write(&self, block: &Block) -> Result<SpillLocation> {
        self.writer.write(block.clone()).await
    }

    pub async fn read(
        &self,
        spill_location: impl Into<Option<&S3SpillLocation>>,
        key: Option<&EncryptionKey>,
        schema: impl Into<Option<SchemaRef>>,
    ) -> Result<Block> {
        let spill_location = require_present(spill_location.into(), "spillLocation")?;
        let schema = schema.into();
        tracing::debug!("write: Started reading block from S3");
        let bytes = self
            .writer
            .amazon_s3
            .get_object(GetObjectRequest::new(
                spill_location.bucket(),
                spill_location.key(),
            ))
            .await?;
        tracing::debug!("write: Completed reading block from S3");
        let schema = require_present(schema, "schema")?;
        let block_crypto = Arc::clone(&self.writer.block_crypto);
        let key = key.cloned();
        let block =
            run_blocking(move || block_crypto.decrypt_with_schema(key.as_ref(), &bytes, schema))
                .await?;
        tracing::debug!("write: Completed decrypting block of size.");
        Ok(block)
    }

    async fn spill_block(&self, block: Block) -> Result<()> {
        if let Some(pool) = &self.async_spill_pool {
            if pool.shutdown.load(Ordering::SeqCst) {
                return Err(FederationError::internal(
                    "The spill pool has been shut down.",
                ));
            }
            let read_guard = self.spill_lock.clone().read_owned().await;
            let slot = pool
                .slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|e| FederationError::internal(e.to_string()))?;
            let workers = pool.workers.clone();
            let writer = self.writer.clone();
            let spill_location = writer.make_spill_location();
            let handle = tokio::spawn(async move {
                let _read_guard = read_guard;
                let _slot = slot;
                let Ok(_worker) = workers.acquire_owned().await else {
                    return;
                };
                if let Ok(spill_location) = writer.write_to(spill_location, block).await {
                    lock(&writer.spill_locations).push(spill_location);
                }
            });
            let mut tasks = lock(&pool.tasks);
            tasks.retain(|t| !t.is_finished());
            tasks.push(handle);
            Ok(())
        } else {
            let spill_location = self.writer.write(block).await?;
            lock(&self.writer.spill_locations).push(spill_location);
            Ok(())
        }
    }

    fn take_async_exception(&self) -> Option<FederationError> {
        lock(&self.writer.async_exception).clone()
    }
}

#[async_trait]
impl BlockSpiller for S3BlockSpiller {
    async fn write_rows(&self, row_writer: &mut (dyn RowWriter + Send)) -> Result<()> {
        let to_spill = {
            let mut slot = lock(&self.in_progress_block);
            let block = self.ensure_init(&mut slot);
            let row_count = block.row_count();
            let rows = row_writer.write_rows(block, row_count)?;
            if rows > 0 {
                block.set_row_count(row_count + rows);
            }
            if len_to_long(block.get_size()) > self.writer.spill_config.max_block_bytes() {
                tracing::info!(
                    "writeRow: Spilling block with {} rows and {} bytes and config {} bytes",
                    block.row_count(),
                    block.get_size(),
                    self.writer.spill_config.max_block_bytes()
                );
                Some(std::mem::replace(block, self.new_block()))
            } else {
                None
            }
        };
        if let Some(block) = to_spill {
            self.spill_block(block).await?;
        }
        Ok(())
    }

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        self.constraint_evaluator.clone()
    }

    async fn spilled(&self) -> Result<bool> {
        if let Some(ex) = self.take_async_exception() {
            return Err(ex);
        }
        let _write_guard = self.spill_lock.write().await;
        let mut slot = lock(&self.in_progress_block);
        let block = self.ensure_init(&mut slot);
        Ok(!lock(&self.writer.spill_locations).is_empty()
            || len_to_long(block.get_size()) >= self.writer.spill_config.max_inline_block_size())
    }

    async fn get_block(&self) -> Result<Block> {
        if self.spilled().await? {
            return Err(FederationError::internal(
                "Blocks have spilled, calls to getBlock not permitted. use getSpillLocations instead.",
            ));
        }
        let mut slot = lock(&self.in_progress_block);
        let block = self.ensure_init(&mut slot);
        tracing::info!(
            "getBlock: Inline Block size[{}] bytes vs {}",
            block.get_size(),
            self.writer.spill_config.max_inline_block_size()
        );
        Ok(block.clone())
    }

    async fn get_spill_locations(&self) -> Result<Vec<SpillLocation>> {
        if !self.spilled().await? {
            return Err(FederationError::internal(
                "Blocks have not spilled, calls to getSpillLocations not permitted. use getBlock instead.",
            ));
        }
        let final_block = {
            let mut slot = lock(&self.in_progress_block);
            let block = self.ensure_init(&mut slot);
            if block.row_count() > 0 {
                tracing::info!(
                    "getSpillLocations: Spilling final block with {} rows and {} bytes and config {} bytes",
                    block.row_count(),
                    block.get_size(),
                    self.writer.spill_config.max_block_bytes()
                );
                Some(std::mem::replace(block, self.new_block()))
            } else {
                None
            }
        };
        if let Some(block) = final_block {
            self.spill_block(block).await?;
        }
        let _write_guard = self.spill_lock.write().await;
        if let Some(ex) = self.take_async_exception() {
            return Err(ex);
        }
        Ok(lock(&self.writer.spill_locations).clone())
    }

    async fn close(&self) {
        tracing::info!(
            "close: Spilled a total of {} bytes in {} ms",
            self.writer.total_bytes_spilled.load(Ordering::SeqCst),
            self.start_time.elapsed().as_millis()
        );
        let Some(pool) = &self.async_spill_pool else {
            return;
        };
        pool.shutdown.store(true, Ordering::SeqCst);
        let tasks = std::mem::take(&mut *lock(&pool.tasks));
        let aborts: Vec<_> = tasks
            .iter()
            .map(tokio::task::JoinHandle::abort_handle)
            .collect();
        let all = async {
            for task in tasks {
                let _ = task.await;
            }
        };
        if tokio::time::timeout(Duration::from_millis(ASYNC_SHUTDOWN_MILLIS), all)
            .await
            .is_err()
        {
            for abort in aborts {
                abort.abort();
            }
        }
    }
}
