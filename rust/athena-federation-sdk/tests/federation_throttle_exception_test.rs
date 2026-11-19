use athena_federation_sdk::exceptions::FederationThrottleException;
use athena_federation_sdk::FederationError;

const ERROR_MESSAGE: &str = "An error occurred";

#[test]
fn test_with_default_constructor() {
    let actual = FederationThrottleException::default();
    assert!(actual.message.is_none());
    assert!(actual.cause.is_none());
}

#[test]
fn test_with_error_message() {
    let actual = FederationThrottleException::new(ERROR_MESSAGE);
    assert_eq!(Some(ERROR_MESSAGE), actual.message.as_deref());
    assert!(actual.cause.is_none());
}

#[test]
fn test_with_error_message_and_throwable() {
    let actual = FederationThrottleException::new(ERROR_MESSAGE)
        .with_cause(FederationError::runtime("Throwable"));
    assert_eq!(Some(ERROR_MESSAGE), actual.message.as_deref());
    assert_eq!(
        Some("Throwable".to_string()),
        actual.cause.as_ref().map(ToString::to_string)
    );
}
