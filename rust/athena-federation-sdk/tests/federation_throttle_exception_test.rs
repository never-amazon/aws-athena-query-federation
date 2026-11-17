use std::sync::Arc;

use athena_federation_sdk::exceptions::FederationThrottleException;

const ERROR_MESSAGE: &str = "An error occurred";

#[derive(Debug)]
struct Throwable;

impl std::fmt::Display for Throwable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Throwable")
    }
}

impl std::error::Error for Throwable {}

#[test]
fn test_with_default_constructor() {
    let actual = FederationThrottleException::new();
    assert!(actual.get_message().is_none());
    assert!(actual.get_cause().is_none());
    assert_eq!(0, actual.get_suppressed().len());
}

#[test]
fn test_with_error_message() {
    let actual = FederationThrottleException::with_message(ERROR_MESSAGE);
    assert_eq!(Some(ERROR_MESSAGE), actual.get_message());
    assert!(actual.get_cause().is_none());
    assert_eq!(0, actual.get_suppressed().len());
}

#[test]
fn test_with_error_message_and_throwable() {
    let cause: Arc<dyn std::error::Error + Send + Sync> = Arc::new(Throwable);
    let actual = FederationThrottleException::with_message_and_cause(ERROR_MESSAGE, cause.clone());
    assert_eq!(Some(ERROR_MESSAGE), actual.get_message());
    assert_eq!(0, actual.get_suppressed().len());
    assert!(Arc::ptr_eq(actual.get_cause().expect("cause"), &cause));
}
