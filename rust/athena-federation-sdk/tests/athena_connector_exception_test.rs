use athena_federation_sdk::exceptions::{AthenaConnectorException, FederationSourceErrorCode};
use athena_federation_sdk::FederationError;

const ERROR_MESSAGE: &str = "Test Message";
const ERROR_CODE: FederationSourceErrorCode = FederationSourceErrorCode::InternalServiceException;
const TEST_RESPONSE: &str = "Test Response";

fn set_up() -> AthenaConnectorException {
    AthenaConnectorException::new(ERROR_CODE, ERROR_MESSAGE).with_details_message(ERROR_MESSAGE)
}

fn assert_details(exception: &AthenaConnectorException) {
    assert_eq!(ERROR_CODE, exception.code, "Error code should match");
    assert_eq!(
        Some(ERROR_MESSAGE),
        exception.details_message.as_deref(),
        "Error details message should match"
    );
}

#[test]
fn constructor_with_response_message_and_error_details_should_set_fields_correctly() {
    let exception = set_up().with_response(FederationError::Runtime(TEST_RESPONSE.to_string()));
    assert_eq!(
        ERROR_MESSAGE, exception.message,
        "Error message should match"
    );
    assert_eq!(
        Some(TEST_RESPONSE.to_string()),
        exception.response.as_ref().map(ToString::to_string),
        "Response should match"
    );
    assert_details(&exception);
    assert!(
        exception.cause.is_none(),
        "Expected no cause for (message, details) constructor"
    );
}

#[test]
fn constructor_with_message_and_error_details_should_set_fields_correctly() {
    let exception = set_up();
    assert_eq!(
        ERROR_MESSAGE, exception.message,
        "Error message should match"
    );
    assert!(
        exception.response.is_none(),
        "Response should be null when not provided"
    );
    assert_details(&exception);
    assert!(
        exception.cause.is_none(),
        "Expected no cause for (message, details) constructor"
    );
}

#[test]
fn constructor_with_response_message_cause_and_error_details_should_set_fields_correctly() {
    let exception = set_up()
        .with_response(FederationError::Runtime(TEST_RESPONSE.to_string()))
        .with_cause(FederationError::Runtime("Test Cause".to_string()));
    assert_eq!(
        ERROR_MESSAGE, exception.message,
        "Error message should match"
    );
    assert_eq!(
        Some("Test Cause".to_string()),
        exception.cause.as_ref().map(ToString::to_string),
        "Cause should match"
    );
    assert_eq!(
        Some(TEST_RESPONSE.to_string()),
        exception.response.as_ref().map(ToString::to_string),
        "Response should match"
    );
    assert_details(&exception);
}
