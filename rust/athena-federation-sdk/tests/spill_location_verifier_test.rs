use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use athena_federation_sdk::domain::spill::{BucketState, HeadBucketClient, SpillLocationVerifier};
use athena_federation_sdk::exceptions::{FederationError, FederationSourceErrorCode};
use athena_federation_sdk::security::{AwsService, AwsServiceError};

struct MockS3 {
    buckets: Vec<String>,
    calls: Arc<AtomicUsize>,
}

impl HeadBucketClient for MockS3 {
    async fn head_bucket(&self, bucket: &str) -> Result<(), AwsServiceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.buckets.iter().any(|b| b == bucket) {
            return Ok(());
        }
        if bucket == "forbidden" {
            Err(AwsServiceError::service(AwsService::S3, None, Some("Forbidden".into())).with_status_code(403))
        } else {
            Err(AwsServiceError::service(AwsService::S3, None, Some("Not Found".into())).with_status_code(404))
        }
    }
}

fn setup() -> (SpillLocationVerifier<MockS3>, Vec<String>, Arc<AtomicUsize>) {
    let bucket_names = vec!["bucket1".to_string(), "bucket2".to_string(), "bucket3".to_string()];
    let calls = Arc::new(AtomicUsize::new(0));
    let mock = MockS3 {
        buckets: bucket_names.clone(),
        calls: calls.clone(),
    };
    (SpillLocationVerifier::new(mock), bucket_names, calls)
}

fn invalid_message(bucket: &str) -> String {
    format!(
        "spill_bucket: \"{}\" not found under your account. Please make sure you have access to the bucket and spill_bucket input has no trailing '/'",
        bucket
    )
}

#[tokio::test]
async fn check_bucket_auth_z() {
    let (mut spy_verifier, bucket_names, update_bucket_state_calls) = setup();
    let index = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as usize)
        % bucket_names.len();

    assert!(
        spy_verifier.check_bucket_auth_z(Some(&bucket_names[index])).await.is_ok(),
        "checkBucketAuthZ failed"
    );
    assert_eq!(1, update_bucket_state_calls.load(Ordering::SeqCst));
    assert_eq!(BucketState::Valid, spy_verifier.get_state());
    assert!(spy_verifier.pass_or_fail().is_ok());

    assert!(
        spy_verifier.check_bucket_auth_z(Some(&bucket_names[index])).await.is_ok(),
        "checkBucketAuthZ failed"
    );
    assert_eq!(1, update_bucket_state_calls.load(Ordering::SeqCst));
    assert_eq!(BucketState::Valid, spy_verifier.get_state());

    assert!(
        spy_verifier.check_bucket_auth_z(Some("")).await.is_ok(),
        "checkBucketAuthZ failed"
    );
    assert_eq!(1, update_bucket_state_calls.load(Ordering::SeqCst));
    assert_eq!(Some(bucket_names[index].as_str()), spy_verifier.get_bucket());
    assert_eq!(BucketState::Valid, spy_verifier.get_state());
}

#[tokio::test]
async fn check_bucket_auth_z_fail() {
    let (mut spy_verifier, _, update_bucket_state_calls) = setup();
    let bucket_not_own = "forbidden";

    let err = spy_verifier
        .check_bucket_auth_z(Some(bucket_not_own))
        .await
        .expect_err("checkBucketAuthZFail failed");
    assert_eq!(invalid_message(bucket_not_own), err.to_string());
    assert_eq!(1, update_bucket_state_calls.load(Ordering::SeqCst));
    assert_eq!(BucketState::Invalid, spy_verifier.get_state());
    assert_eq!(
        invalid_message(bucket_not_own),
        spy_verifier.pass_or_fail().expect_err("passOrFail should fail").to_string()
    );

    let err = spy_verifier
        .check_bucket_auth_z(Some(bucket_not_own))
        .await
        .expect_err("checkBucketAuthZFail failed");
    assert_eq!(invalid_message(bucket_not_own), err.to_string());
    assert_eq!(1, update_bucket_state_calls.load(Ordering::SeqCst));
}

struct FailingS3 {
    error: AwsServiceError,
}

impl HeadBucketClient for FailingS3 {
    async fn head_bucket(&self, _bucket: &str) -> Result<(), AwsServiceError> {
        Err(self.error.clone())
    }
}

#[tokio::test]
async fn client_error_passes_through_unchanged() {
    let error = AwsServiceError::client("Unable to execute HTTP request: Connect timed out");
    let mut verifier = SpillLocationVerifier::new(FailingS3 { error: error.clone() });
    let result = verifier.check_bucket_auth_z(Some("bucket1")).await;
    match result {
        Err(FederationError::AwsService(e)) => assert_eq!(error, e),
        other => panic!("expected the client error unchanged, got {:?}", other),
    }
    assert_eq!(BucketState::Unchecked, verifier.get_state());
}

#[tokio::test]
async fn s3_error_other_than_403_or_404_raises_access_denied() {
    let error = AwsServiceError::service(AwsService::S3, Some("InternalError".into()), Some("We encountered an internal error.".into()))
        .with_error_details()
        .with_status_code(500);
    let mut verifier = SpillLocationVerifier::new(FailingS3 { error });
    let e = verifier.check_bucket_auth_z(Some("bucket1")).await.unwrap_err();
    assert_eq!(Some(FederationSourceErrorCode::AccessDeniedException), e.error_code());
    assert_eq!("Error while checking bucket ownership for bucket1", e.to_string());
}
