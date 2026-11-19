use std::fmt;
use std::future::Future;

use crate::exceptions::{
    AthenaConnectorException, FederationError, FederationSourceErrorCode, Result,
};
use crate::security::{AwsService, AwsServiceError};

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
