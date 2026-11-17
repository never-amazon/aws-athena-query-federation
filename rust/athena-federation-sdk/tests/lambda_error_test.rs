use athena_federation_sdk::exceptions::{FederationSourceErrorCode, JavaExceptionKind};
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
