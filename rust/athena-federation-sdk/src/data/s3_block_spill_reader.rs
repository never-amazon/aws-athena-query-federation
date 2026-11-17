use std::sync::Arc;

use arrow::datatypes::SchemaRef;

use super::block::Block;
use super::block_allocator::BlockAllocator;
use super::s3_client::{GetObjectRequest, S3Client};
use crate::domain::spill::S3SpillLocation;
use crate::exceptions::{FederationError, Result};
use crate::security::{
    run_blocking, AesGcmBlockCrypto, BlockCrypto, EncryptionKey, NoOpBlockCrypto,
};

pub(crate) fn require_present<T>(value: Option<T>, name: &str) -> Result<T> {
    value.ok_or_else(|| FederationError::null_pointer(format!("{name} was null")))
}

pub struct S3BlockSpillReader {
    amazon_s3: Arc<dyn S3Client>,
    allocator: Arc<dyn BlockAllocator>,
}

impl S3BlockSpillReader {
    pub fn new(
        amazon_s3: Arc<dyn S3Client>,
        allocator: Arc<dyn BlockAllocator>,
    ) -> S3BlockSpillReader {
        S3BlockSpillReader {
            amazon_s3,
            allocator,
        }
    }

    fn block_crypto(&self, key: Option<&EncryptionKey>) -> Box<dyn BlockCrypto> {
        if key.is_some() {
            Box::new(AesGcmBlockCrypto::new(self.allocator.clone()))
        } else {
            Box::new(NoOpBlockCrypto::new(self.allocator.clone()))
        }
    }

    async fn get_object(&self, spill_location: &S3SpillLocation) -> Result<Vec<u8>> {
        tracing::debug!("read: Started reading block from S3");
        let bytes = self
            .amazon_s3
            .get_object(GetObjectRequest::new(
                spill_location.bucket(),
                spill_location.key(),
            ))
            .await?;
        tracing::debug!("read: Completed reading block from S3");
        Ok(bytes)
    }

    pub async fn read_with_schema(
        &self,
        spill_location: impl Into<Option<&S3SpillLocation>>,
        key: Option<&EncryptionKey>,
        schema: impl Into<Option<SchemaRef>>,
    ) -> Result<Block> {
        let spill_location = require_present(spill_location.into(), "spillLocation")?;
        let schema = schema.into();
        let bytes = self.get_object(spill_location).await?;
        let schema = require_present(schema, "schema")?;
        let block_crypto = self.block_crypto(key);
        let key = key.cloned();
        let block =
            run_blocking(move || block_crypto.decrypt_with_schema(key.as_ref(), &bytes, schema))
                .await?;
        tracing::debug!("read: Completed decrypting block of size.");
        Ok(block)
    }

    pub async fn read(
        &self,
        spill_location: impl Into<Option<&S3SpillLocation>>,
        key: Option<&EncryptionKey>,
    ) -> Result<Vec<u8>> {
        let spill_location = require_present(spill_location.into(), "spillLocation")?;
        let bytes = self.get_object(spill_location).await?;
        let block_crypto = self.block_crypto(key);
        let key = key.cloned();
        run_blocking(move || block_crypto.decrypt(key.as_ref(), &bytes)).await
    }
}
