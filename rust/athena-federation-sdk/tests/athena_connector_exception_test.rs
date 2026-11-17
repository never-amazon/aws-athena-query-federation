use std::sync::Arc;

use athena_federation_sdk::exceptions::{AthenaConnectorException, ErrorDetails};

const ERROR_MESSAGE: &str = "Test Message";
const ERROR_CODE: &str = "TestErrorCode";
const TEST_RESPONSE: &str = "Test Response";

#[derive(Debug)]
struct TestCause(String);

impl std::fmt::Display for TestCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TestCause {}

fn set_up() -> ErrorDetails {
    ErrorDetails::builder()
        .error_code(ERROR_CODE)
        .error_message(ERROR_MESSAGE)
        .build()
}

#[test]
fn constructor_with_response_message_and_error_details_should_set_fields_correctly() {
    let error_details = set_up();
    let exception = AthenaConnectorException::with_response(
        Arc::new(TEST_RESPONSE),
        ERROR_MESSAGE,
        error_details.clone(),
    );
    assert_eq!(
        ERROR_MESSAGE,
        exception.message(),
        "Error message should match"
    );
    assert_eq!(
        Some(&TEST_RESPONSE),
        exception.response().and_then(|r| r.downcast_ref::<&str>()),
        "Response should match"
    );
    assert_eq!(
        &error_details,
        exception.error_details(),
        "Error details should match"
    );
    assert!(
        exception.cause().is_none(),
        "Expected no cause for (message, details) constructor"
    );
}

#[test]
fn constructor_with_message_and_error_details_should_set_fields_correctly() {
    let error_details = set_up();
    let exception = AthenaConnectorException::new(ERROR_MESSAGE, error_details.clone());
    assert_eq!(
        ERROR_MESSAGE,
        exception.message(),
        "Error message should match"
    );
    assert!(
        exception.response().is_none(),
        "Response should be null when not provided"
    );
    assert_eq!(
        &error_details,
        exception.error_details(),
        "Error details should match"
    );
    assert!(
        exception.cause().is_none(),
        "Expected no cause for (message, details) constructor"
    );
}

#[test]
fn constructor_with_response_message_cause_and_error_details_should_set_fields_correctly() {
    let error_details = set_up();
    let test_cause: Arc<dyn std::error::Error + Send + Sync> =
        Arc::new(TestCause("Test Cause".to_string()));
    let exception = AthenaConnectorException::with_response_and_cause(
        Arc::new(TEST_RESPONSE),
        ERROR_MESSAGE,
        test_cause.clone(),
        error_details.clone(),
    );
    assert_eq!(
        ERROR_MESSAGE,
        exception.message(),
        "Error message should match"
    );
    assert!(
        exception
            .cause()
            .is_some_and(|c| Arc::ptr_eq(c, &test_cause)),
        "Cause should match"
    );
    assert_eq!(
        Some(&TEST_RESPONSE),
        exception.response().and_then(|r| r.downcast_ref::<&str>()),
        "Response should match"
    );
    assert_eq!(
        &error_details,
        exception.error_details(),
        "Error details should match"
    );
}
