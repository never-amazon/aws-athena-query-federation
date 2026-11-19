use std::fmt;
use std::future::Future;

use crate::exceptions::{
    AthenaConnectorException, FederationError, FederationSourceErrorCode, Result,
};
use crate::security::{AwsService, AwsServiceError};

const SEPARATOR: &str = "/";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct S3SpillLocation {
    bucket: Option<String>,
    key: String,
    directory: bool,
}

impl S3SpillLocation {
    pub fn new(
        bucket: impl Into<String>,
        key: impl Into<String>,
        directory: bool,
    ) -> S3SpillLocation {
        S3SpillLocation {
            bucket: Some(bucket.into()),
            key: key.into(),
            directory,
        }
    }

    pub fn bucket(&self) -> &str {
        or_null(self.bucket.as_deref())
    }

    pub fn bucket_value(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    pub fn new_builder() -> S3SpillLocationBuilder {
        S3SpillLocationBuilder::default()
    }
}

impl fmt::Display for S3SpillLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "S3SpillLocation{{bucket='{}', key='{}', directory={}}}",
            self.bucket(),
            self.key,
            self.directory
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct S3SpillLocationBuilder {
    bucket: Option<String>,
    prefix: Option<String>,
    query_id: Option<String>,
    split_id: Option<String>,
}

fn or_null(v: Option<&str>) -> &str {
    v.unwrap_or("null")
}

impl S3SpillLocationBuilder {
    #[must_use]
    pub fn with_bucket(mut self, bucket: impl Into<String>) -> Self {
        self.bucket = Some(bucket.into());
        self
    }

    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub fn with_query_id(mut self, query_id: impl Into<String>) -> Self {
        self.query_id = Some(query_id.into());
        self
    }

    #[must_use]
    pub fn with_split_id(mut self, split_id: impl Into<String>) -> Self {
        self.split_id = Some(split_id.into());
        self
    }

    pub fn build(&self) -> S3SpillLocation {
        let query_blank = self.query_id.as_deref().is_none_or(|q| q.trim().is_empty());
        let bucket_prefix = if query_blank {
            or_null(self.prefix.as_deref()).to_string()
        } else {
            format!(
                "{}{}{}",
                or_null(self.prefix.as_deref()),
                SEPARATOR,
                or_null(self.query_id.as_deref())
            )
        };
        let key = format!(
            "{}{}{}",
            bucket_prefix,
            SEPARATOR,
            or_null(self.split_id.as_deref())
        );
        S3SpillLocation {
            bucket: self.bucket.clone(),
            key,
            directory: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpillLocation {
    S3(S3SpillLocation),
}

impl SpillLocation {
    pub fn type_name(&self) -> &'static str {
        match self {
            SpillLocation::S3(_) => "S3SpillLocation",
        }
    }

    pub fn as_s3(&self) -> Option<&S3SpillLocation> {
        match self {
            SpillLocation::S3(s) => Some(s),
        }
    }
}

impl From<S3SpillLocation> for SpillLocation {
    fn from(s: S3SpillLocation) -> Self {
        SpillLocation::S3(s)
    }
}

impl fmt::Display for SpillLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpillLocation::S3(s) => write!(f, "{s}"),
        }
    }
}

pub trait HeadBucketClient: Send + Sync {
    fn head_bucket(
        &self,
        bucket: &str,
    ) -> impl Future<Output = std::result::Result<(), AwsServiceError>> + Send;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketState {
    Unchecked,
    Valid,
    Invalid,
}

impl fmt::Display for BucketState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BucketState::Unchecked => "UNCHECKED",
            BucketState::Valid => "VALID",
            BucketState::Invalid => "INVALID",
        })
    }
}

pub struct SpillLocationVerifier<C: HeadBucketClient> {
    amazons3: C,
    bucket: Option<String>,
    state: BucketState,
}

impl<C: HeadBucketClient> SpillLocationVerifier<C> {
    pub fn new(amazons3: C) -> Self {
        SpillLocationVerifier {
            amazons3,
            bucket: None,
            state: BucketState::Unchecked,
        }
    }

    pub async fn check_bucket_auth_z(&mut self, spill_bucket: Option<&str>) -> Result<()> {
        let spill_bucket = match spill_bucket {
            None | Some("") => return Ok(()),
            Some(b) => b,
        };
        if self.bucket.as_deref() != Some(spill_bucket) {
            tracing::info!(
                "Spill bucket has been changed from {:?} to {}",
                self.bucket,
                spill_bucket
            );
            self.bucket = Some(spill_bucket.to_string());
            self.state = BucketState::Unchecked;
        }
        if self.state == BucketState::Unchecked {
            self.update_bucket_state().await?;
        }
        self.pass_or_fail()
    }

    pub async fn update_bucket_state(&mut self) -> Result<()> {
        let bucket = self.bucket.clone().unwrap_or_default();
        let result = match self.amazons3.head_bucket(&bucket).await {
            Ok(()) => {
                self.state = BucketState::Valid;
                Ok(())
            }
            Err(ex) if !ex.is_service_exception_of(AwsService::S3) => {
                Err(FederationError::from(ex))
            }
            Err(ex) if matches!(ex.status_code(), Some(404 | 403)) => {
                self.state = BucketState::Invalid;
                Ok(())
            }
            Err(ex) => Err(FederationError::AthenaConnector(
                AthenaConnectorException::new(
                    FederationSourceErrorCode::AccessDeniedException,
                    format!("Error while checking bucket ownership for {bucket}"),
                )
                .with_response(FederationError::AwsService(ex)),
            )),
        };
        tracing::info!(
            "The state of bucket {} has been updated to {} from {}",
            bucket,
            self.state,
            BucketState::Unchecked
        );
        result
    }

    pub fn pass_or_fail(&self) -> Result<()> {
        match self.state {
            BucketState::Unchecked => Err(FederationError::internal("Bucket state should have been checked already.")),
            BucketState::Invalid => Err(FederationError::invalid_input(format!(
                "spill_bucket: \"{}\" not found under your account. Please make sure you have access to the bucket and spill_bucket input has no trailing '/'",
                self.bucket.as_deref().unwrap_or("null")
            ))),
            BucketState::Valid => Ok(()),
        }
    }

    pub fn state(&self) -> BucketState {
        self.state
    }

    pub fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }
}
