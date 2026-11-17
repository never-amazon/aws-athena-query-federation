use std::fmt;

use crate::exceptions::{AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3ServiceError {
    pub status_code: i32,
    pub message: String,
}

impl fmt::Display for S3ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (Status Code: {})", self.message, self.status_code)
    }
}

impl std::error::Error for S3ServiceError {}

pub trait HeadBucketClient: Send + Sync {
    fn head_bucket(&self, bucket: &str) -> std::result::Result<(), S3ServiceError>;
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
    update_bucket_state_calls: usize,
    pass_or_fail_calls: usize,
}

impl<C: HeadBucketClient> SpillLocationVerifier<C> {
    pub fn new(amazons3: C) -> Self {
        SpillLocationVerifier {
            amazons3,
            bucket: None,
            state: BucketState::Unchecked,
            update_bucket_state_calls: 0,
            pass_or_fail_calls: 0,
        }
    }

    pub fn check_bucket_auth_z(&mut self, spill_bucket: Option<&str>) -> Result<()> {
        let spill_bucket = match spill_bucket {
            None | Some("") => return Ok(()),
            Some(b) => b,
        };
        if self.bucket.as_deref() != Some(spill_bucket) {
            tracing::info!("Spill bucket has been changed from {:?} to {}", self.bucket, spill_bucket);
            self.bucket = Some(spill_bucket.to_string());
            self.state = BucketState::Unchecked;
        }
        if self.state == BucketState::Unchecked {
            self.update_bucket_state()?;
        }
        self.pass_or_fail()
    }

    pub fn update_bucket_state(&mut self) -> Result<()> {
        self.update_bucket_state_calls += 1;
        let bucket = self.bucket.clone().unwrap_or_default();
        let result = match self.amazons3.head_bucket(&bucket) {
            Ok(()) => {
                self.state = BucketState::Valid;
                Ok(())
            }
            Err(ex) if ex.status_code == 404 || ex.status_code == 403 => {
                self.state = BucketState::Invalid;
                Ok(())
            }
            Err(ex) => Err(FederationError::AthenaConnector(AthenaConnectorException::with_cause(
                format!("Error while checking bucket ownership for {}", bucket),
                std::sync::Arc::new(ex),
                ErrorDetails::of(FederationSourceErrorCode::AccessDeniedException),
            ))),
        };
        tracing::info!(
            "The state of bucket {} has been updated to {} from {}",
            bucket,
            self.state,
            BucketState::Unchecked
        );
        result
    }

    pub fn pass_or_fail(&mut self) -> Result<()> {
        self.pass_or_fail_calls += 1;
        match self.state {
            BucketState::Unchecked => Err(FederationError::internal("Bucket state should have been checked already.")),
            BucketState::Invalid => Err(FederationError::invalid_input(format!(
                "spill_bucket: \"{}\" not found under your account. Please make sure you have access to the bucket and spill_bucket input has no trailing '/'",
                self.bucket.as_deref().unwrap_or("null")
            ))),
            BucketState::Valid => Ok(()),
        }
    }

    pub fn get_state(&self) -> BucketState {
        self.state
    }

    pub fn get_bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub fn update_bucket_state_calls(&self) -> usize {
        self.update_bucket_state_calls
    }

    pub fn pass_or_fail_calls(&self) -> usize {
        self.pass_or_fail_calls
    }
}
