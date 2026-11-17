use std::sync::Arc;

use arrow::datatypes::Schema;
use arrow::record_batch::RecordBatch;
use async_trait::async_trait;
use athena_federation_sdk::data::{GetObjectRequest, S3Client};
use athena_federation_sdk::security::{AwsService, AwsServiceError};
use athena_federation_sdk::{FederationError, Result};
use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;

use crate::snowflake_constants::SnowflakeConstants;

pub const EXPORT_READ_BATCH_SIZE: usize = 32768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ObjectSummary {
    key: String,
}

impl S3ObjectSummary {
    pub fn new(key: impl Into<String>) -> S3ObjectSummary {
        S3ObjectSummary { key: key.into() }
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[async_trait]
pub trait S3ObjectLister: Send + Sync {
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> std::result::Result<Vec<S3ObjectSummary>, AwsServiceError>;
}

#[async_trait]
impl S3ObjectLister for aws_sdk_s3::Client {
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: &str,
    ) -> std::result::Result<Vec<S3ObjectSummary>, AwsServiceError> {
        let output = self
            .list_objects()
            .bucket(bucket)
            .prefix(prefix)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(AwsService::S3, &e))?;
        Ok(output
            .contents()
            .iter()
            .map(|object| S3ObjectSummary::new(object.key().unwrap_or_default()))
            .collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Uri {
    bucket: Option<String>,
    key: Option<String>,
}

impl S3Uri {
    pub fn parse(uri: &str) -> Result<S3Uri> {
        let Some(rest) = uri.strip_prefix("s3://") else {
            return Err(FederationError::illegal_argument(format!(
                "Provided URI is not a valid S3 URI: {uri}"
            )));
        };
        let (bucket, key) = match rest.split_once('/') {
            Some((bucket, key)) => (bucket, key),
            None => (rest, ""),
        };
        Ok(S3Uri {
            bucket: (!bucket.is_empty()).then(|| bucket.to_string()),
            key: (!key.is_empty()).then(|| key.to_string()),
        })
    }

    pub fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
}

pub fn construct_s3_uri(bucket: &str, key: &str) -> String {
    format!("s3://{bucket}/{key}")
}

pub type BatchIterator = Box<dyn Iterator<Item = Result<RecordBatch>> + Send>;

#[async_trait]
pub trait ExportReader: Send + Sync {
    async fn inspect_parquet_schema(&self, uri: &str) -> Result<Schema>;

    async fn construct_arrow_reader(&self, uri: &str, schema: &Schema) -> Result<BatchIterator>;
}

pub struct S3ParquetExportReader {
    s3: Arc<dyn S3Client>,
}

impl S3ParquetExportReader {
    pub fn new(s3: Arc<dyn S3Client>) -> S3ParquetExportReader {
        S3ParquetExportReader { s3 }
    }

    async fn fetch(&self, uri: &str) -> Result<Bytes> {
        let parsed = S3Uri::parse(uri)?;
        let request = GetObjectRequest::new(
            parsed.bucket().unwrap_or_default(),
            parsed.key().unwrap_or_default(),
        );
        let bytes = self
            .s3
            .get_object(request)
            .await
            .map_err(FederationError::AwsService)?;
        Ok(Bytes::from(bytes))
    }
}

fn parquet_error(error: impl std::fmt::Display) -> FederationError {
    FederationError::Runtime(error.to_string())
}

#[async_trait]
impl ExportReader for S3ParquetExportReader {
    async fn inspect_parquet_schema(&self, uri: &str) -> Result<Schema> {
        let bytes = self.fetch(uri).await?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(bytes).map_err(parquet_error)?;
        Ok(builder.schema().as_ref().clone())
    }

    async fn construct_arrow_reader(&self, uri: &str, schema: &Schema) -> Result<BatchIterator> {
        tracing::debug!("URI {}", uri);
        let bytes = self.fetch(uri).await?;
        let builder = ParquetRecordBatchReaderBuilder::try_new(bytes).map_err(parquet_error)?;
        let wanted: Vec<usize> = builder
            .schema()
            .fields()
            .iter()
            .enumerate()
            .filter(|(_, field)| {
                !field
                    .name()
                    .eq_ignore_ascii_case(SnowflakeConstants::BLOCK_PARTITION_COLUMN_NAME)
                    && schema.field_with_name(field.name()).is_ok()
            })
            .map(|(index, _)| index)
            .collect();
        let mask = ProjectionMask::roots(builder.parquet_schema(), wanted);
        let reader = builder
            .with_projection(mask)
            .with_batch_size(EXPORT_READ_BATCH_SIZE)
            .build()
            .map_err(parquet_error)?;
        Ok(Box::new(reader.map(|batch| batch.map_err(parquet_error))))
    }
}
