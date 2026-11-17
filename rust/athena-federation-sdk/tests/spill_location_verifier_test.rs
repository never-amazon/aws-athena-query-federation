use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use athena_federation_sdk::domain::spill::{HeadBucketClient, S3ServiceError, SpillLocationVerifier};

struct MockS3 {
    buckets: Vec<String>,
    calls: Arc<AtomicUsize>,
}

impl HeadBucketClient for MockS3 {
    fn head_bucket(&self, bucket: &str) -> Result<(), S3ServiceError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.buckets.iter().any(|b| b == bucket) {
            return Ok(());
        }
        if bucket == "forbidden" {
            Err(S3ServiceError { status_code: 403, message: "Forbidden".into() })
        } else {
            Err(S3ServiceError { status_code: 404, message: "Not Found".into() })
        }
    }
}

fn setup() -> (SpillLocationVerifier<MockS3>, Vec<String>, Arc<AtomicUsize>) {
    let bucket_names = vec!["bucket1".to_string(), "bucket2".to_string(), "bucket3".to_string()];
    let calls = Arc::new(AtomicUsize::new(0));
    let mock = MockS3 { buckets: bucket_names.clone(), calls: calls.clone() };
    (SpillLocationVerifier::new(mock), bucket_names, calls)
}

#[test]
fn check_bucket_auth_z() {
    let (mut spy_verifier, bucket_names, calls) = setup();
    let index = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) as usize)
        % bucket_names.len();
    assert!(spy_verifier.check_bucket_auth_z(Some(&bucket_names[index])).is_ok(), "checkBucketAuthZ failed");
    assert_eq!(1, spy_verifier.update_bucket_state_calls());
    assert_eq!(1, spy_verifier.pass_or_fail_calls());
    assert!(spy_verifier.check_bucket_auth_z(Some(&bucket_names[index])).is_ok(), "checkBucketAuthZ failed");
    assert_eq!(1, spy_verifier.update_bucket_state_calls());
    assert_eq!(2, spy_verifier.pass_or_fail_calls());
    assert!(spy_verifier.check_bucket_auth_z(Some("")).is_ok(), "checkBucketAuthZ failed");
    assert_eq!(1, spy_verifier.update_bucket_state_calls());
    assert_eq!(2, spy_verifier.pass_or_fail_calls());
    assert_eq!(1, calls.load(Ordering::SeqCst));
}

#[test]
fn check_bucket_auth_z_fail() {
    let (mut spy_verifier, _, _) = setup();
    let bucket_not_own = "forbidden";
    let expected = format!(
        "spill_bucket: \"{}\" not found under your account. Please make sure you have access to the bucket and spill_bucket input has no trailing '/'",
        bucket_not_own
    );
    let err = spy_verifier.check_bucket_auth_z(Some(bucket_not_own)).expect_err("checkBucketAuthZFail failed");
    assert_eq!(expected, err.to_string());
    assert_eq!(1, spy_verifier.update_bucket_state_calls());
    assert_eq!(1, spy_verifier.pass_or_fail_calls());
    let err = spy_verifier.check_bucket_auth_z(Some(bucket_not_own)).expect_err("checkBucketAuthZFail failed");
    assert_eq!(expected, err.to_string());
    assert_eq!(1, spy_verifier.update_bucket_state_calls());
    assert_eq!(2, spy_verifier.pass_or_fail_calls());
}
