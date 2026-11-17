use std::sync::Arc;

use arrow::error::ArrowError;
use athena_federation_sdk::exceptions::{
    AthenaConnectorException, ErrorDetails, FederationSourceErrorCode, JavaExceptionKind,
};
use athena_federation_sdk::lambda::LambdaError;
use athena_federation_sdk::FederationError;

#[test]
fn checked_cause_wrapped_in_runtime_exception_matches_java_report() {
    let checked = FederationError::wrapped(
        JavaExceptionKind::Checked("org.postgresql.util.PSQLException"),
        std::io::Error::other("ERROR: column \"ghost\" does not exist\n  Position: 14"),
    );
    let error = checked.into_runtime_exception();
    assert_eq!(
        "errorType: java.lang.RuntimeException\n\
         errorMessage: org.postgresql.util.PSQLException: ERROR: column \"ghost\" does not exist\\n  Position: 14\n\
         cause: org.postgresql.util.PSQLException: ERROR: column \"ghost\" does not exist\\n  Position: 14\n",
        LambdaError::new(&error).to_report()
    );
}

#[test]
fn connector_exception_report_carries_error_code() {
    let error = FederationError::connector(
        FederationSourceErrorCode::InvalidInputException,
        "Schema name case insensitive match failed. More than one Schema that matches 'no_such_schema'",
    );
    let lambda_error = LambdaError::new(&error);
    assert_eq!(Some("InvalidInputException"), lambda_error.error_code());
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: Schema name case insensitive match failed. More than one Schema that matches 'no_such_schema'\n\
         errorCode: InvalidInputException\n",
        lambda_error.to_report()
    );
}

#[test]
fn runtime_exception_is_not_wrapped_again() {
    let error = FederationError::NumberFormat("For input string: \"abc\"".into());
    let error = error.into_runtime_exception();
    assert_eq!(
        "errorType: java.lang.NumberFormatException\nerrorMessage: For input string: \"abc\"\n",
        LambdaError::new(&error).to_report()
    );
}

#[test]
fn diagnostic_carries_java_type_and_message() {
    let error = FederationError::NumberFormat("For input string: \"abc\"".into());
    let diagnostic: lambda_runtime::Diagnostic = LambdaError::new(&error).into();
    assert_eq!("java.lang.NumberFormatException", diagnostic.error_type);
    assert_eq!("For input string: \"abc\"", diagnostic.error_message);
}

#[test]
fn io_error_wrapped_in_runtime_exception_reports_one_cause() {
    let error = FederationError::from(std::io::Error::other("io boom")).into_runtime_exception();
    assert_eq!(
        "errorType: java.lang.RuntimeException\n\
         errorMessage: java.io.IOException: io boom\n\
         cause: java.io.IOException: io boom\n",
        LambdaError::new(&error).to_report()
    );
}

#[test]
fn connector_exception_with_io_cause_reports_one_cause() {
    let error = FederationError::AthenaConnector(AthenaConnectorException::with_cause(
        "outer",
        Arc::new(FederationError::from(std::io::Error::other("inner"))),
        ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
    ));
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: outer\n\
         errorCode: InternalServiceException\n\
         cause: java.io.IOException: inner\n",
        LambdaError::new(&error).to_report()
    );
}

#[test]
fn arrow_error_cause_reports_one_cause() {
    let error = FederationError::AthenaConnector(AthenaConnectorException::with_cause(
        "outer",
        Arc::new(FederationError::from(ArrowError::IpcError("bad".into()))),
        ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
    ));
    assert_eq!(1, LambdaError::new(&error).causes().len());
}

#[test]
fn null_pointer_without_message_reports_null() {
    let error = FederationError::NullPointer(None);
    let lambda_error = LambdaError::new(&error);
    assert_eq!(None, lambda_error.error_message());
    assert_eq!(
        "errorType: java.lang.NullPointerException\nerrorMessage: null\n",
        lambda_error.to_report()
    );
    let diagnostic: lambda_runtime::Diagnostic = lambda_error.into();
    assert_eq!("java.lang.NullPointerException", diagnostic.error_message);
}
