use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_s3::primitives::ByteStream;

use crate::domain::spill::{HeadBucketClient, S3ServiceError};
use crate::security::{send_with_override, AwsRequestOverrideConfiguration, AwsServiceError};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PutObjectRequest {
    bucket: String,
    key: String,
    content_length: i64,
    metadata: HashMap<String, String>,
    override_configuration: Option<AwsRequestOverrideConfiguration>,
}

impl PutObjectRequest {
    pub fn builder() -> PutObjectRequestBuilder {
        PutObjectRequestBuilder::default()
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn content_length(&self) -> i64 {
        self.content_length
    }

    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    pub fn override_configuration(&self) -> Option<&AwsRequestOverrideConfiguration> {
        self.override_configuration.as_ref()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PutObjectRequestBuilder {
    inner: PutObjectRequest,
}

impl PutObjectRequestBuilder {
    pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
        self.inner.bucket = bucket.into();
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.inner.key = key.into();
        self
    }

    pub fn content_length(mut self, content_length: i64) -> Self {
        self.inner.content_length = content_length;
        self
    }

    pub fn metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.inner.metadata = metadata;
        self
    }

    pub fn override_configuration(mut self, override_configuration: AwsRequestOverrideConfiguration) -> Self {
        self.inner.override_configuration = Some(override_configuration);
        self
    }

    pub fn build(self) -> PutObjectRequest {
        self.inner
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GetObjectRequest {
    bucket: String,
    key: String,
}

impl GetObjectRequest {
    pub fn new(bucket: impl Into<String>, key: impl Into<String>) -> GetObjectRequest {
        GetObjectRequest {
            bucket: bucket.into(),
            key: key.into(),
        }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[async_trait]
pub trait S3Client: Send + Sync {
    async fn put_object(&self, request: PutObjectRequest, body: Vec<u8>) -> Result<(), AwsServiceError>;

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError>;
}

#[async_trait]
impl S3Client for aws_sdk_s3::Client {
    async fn put_object(&self, request: PutObjectRequest, body: Vec<u8>) -> Result<(), AwsServiceError> {
        let mut builder = self
            .put_object()
            .bucket(&request.bucket)
            .key(&request.key)
            .content_length(request.content_length)
            .body(ByteStream::from(body));
        if !request.metadata.is_empty() {
            builder = builder.set_metadata(Some(request.metadata.clone()));
        }
        send_with_override!(builder, request.override_configuration.as_ref(), aws_sdk_s3::config::Builder)
            .map_err(|e| AwsServiceError::from_sdk_error(&e))?;
        Ok(())
    }

    async fn get_object(&self, request: GetObjectRequest) -> Result<Vec<u8>, AwsServiceError> {
        let output = self
            .get_object()
            .bucket(request.bucket)
            .key(request.key)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(&e))?;
        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| AwsServiceError::io(e.to_string()))?;
        Ok(bytes.into_bytes().to_vec())
    }
}

impl HeadBucketClient for aws_sdk_s3::Client {
    async fn head_bucket(&self, bucket: &str) -> Result<(), S3ServiceError> {
        self.head_bucket().bucket(bucket).send().await.map(|_| ()).map_err(|e| {
            let error = AwsServiceError::from_sdk_error(&e);
            S3ServiceError {
                status_code: error.status_code().map(i32::from).unwrap_or(0),
                message: error.message().to_string(),
            }
        })
    }
}
