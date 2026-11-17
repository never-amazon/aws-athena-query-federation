use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use crate::exceptions::{AthenaConnectorException, FederationError};
use crate::security::AwsServiceError;

const RUNTIME_EXCEPTION: &str = "java.lang.RuntimeException";
const IO_EXCEPTION: &str = "java.io.IOException";
const ATHENA_CONNECTOR_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaErrorCause {
    error_type: String,
    error_message: Option<String>,
}

impl LambdaErrorCause {
    pub fn error_type(&self) -> &str {
        &self.error_type
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LambdaError {
    error_type: String,
    error_message: Option<String>,
    error_code: Option<String>,
    causes: Vec<LambdaErrorCause>,
}

impl LambdaError {
    pub fn new(error: &FederationError) -> LambdaError {
        let error_code = error
            .as_connector_exception()
            .map(|e| e.error_details().error_code().unwrap_or("null").to_string());
        let mut causes = Vec::new();
        let mut next = federation_cause(error);
        while let Some(cause) = next {
            causes.push(LambdaErrorCause {
                error_type: java_class_of(cause),
                error_message: java_message_of(cause),
            });
            next = java_cause(cause);
        }
        LambdaError {
            error_type: error.java_class_name(),
            error_message: error.java_message(),
            error_code,
            causes,
        }
    }

    pub fn error_type(&self) -> &str {
        &self.error_type
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn causes(&self) -> &[LambdaErrorCause] {
        &self.causes
    }

    pub fn to_report(&self) -> String {
        let mut text = format!(
            "errorType: {}\nerrorMessage: {}\n",
            self.error_type,
            one_line(self.error_message.as_deref())
        );
        if let Some(code) = &self.error_code {
            text.push_str("errorCode: ");
            text.push_str(code);
            text.push('\n');
        }
        for cause in &self.causes {
            text.push_str("cause: ");
            text.push_str(&cause.error_type);
            text.push_str(": ");
            text.push_str(&one_line(cause.error_message.as_deref()));
            text.push('\n');
        }
        text
    }
}

impl fmt::Display for LambdaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.error_message {
            Some(message) => write!(f, "{}: {message}", self.error_type),
            None => f.write_str(&self.error_type),
        }
    }
}

impl StdError for LambdaError {}

impl From<&FederationError> for LambdaError {
    fn from(error: &FederationError) -> Self {
        LambdaError::new(error)
    }
}

impl From<FederationError> for LambdaError {
    fn from(error: FederationError) -> Self {
        LambdaError::new(&error)
    }
}

impl From<LambdaError> for lambda_runtime::Diagnostic {
    fn from(error: LambdaError) -> Self {
        let error_message = error
            .error_message
            .unwrap_or_else(|| error.error_type.clone());
        lambda_runtime::Diagnostic {
            error_type: error.error_type,
            error_message,
        }
    }
}

fn one_line(message: Option<&str>) -> String {
    message.map_or_else(
        || "null".to_string(),
        |m| m.replace('\r', "\\r").replace('\n', "\\n"),
    )
}

fn java_message_of(error: &(dyn StdError + 'static)) -> Option<String> {
    match as_federation(error) {
        Some(federation) => federation.java_message(),
        None => Some(error.to_string()),
    }
}

fn federation_cause(error: &FederationError) -> Option<&(dyn StdError + 'static)> {
    match error {
        FederationError::Wrapped { source, .. } => source.source(),
        FederationError::IoError(source) => source.source(),
        FederationError::Arrow(source) => source.source(),
        FederationError::RuntimeWrapper { source } => Some(source.as_ref()),
        other => other.source(),
    }
}

fn as_federation<'a>(error: &'a (dyn StdError + 'static)) -> Option<&'a FederationError> {
    error.downcast_ref::<FederationError>().or_else(|| {
        error
            .downcast_ref::<Arc<FederationError>>()
            .map(AsRef::as_ref)
    })
}

fn java_cause<'a>(error: &'a (dyn StdError + 'static)) -> Option<&'a (dyn StdError + 'static)> {
    match as_federation(error) {
        Some(federation) => federation_cause(federation),
        None => error.source(),
    }
}

fn java_class_of(error: &(dyn StdError + 'static)) -> String {
    if let Some(federation) = as_federation(error) {
        return federation.java_class_name();
    }
    if error.is::<AthenaConnectorException>() {
        return ATHENA_CONNECTOR_EXCEPTION.to_string();
    }
    if let Some(aws) = error.downcast_ref::<AwsServiceError>() {
        return FederationError::AwsService(aws.clone()).java_class_name();
    }
    if error.is::<std::io::Error>() {
        return IO_EXCEPTION.to_string();
    }
    RUNTIME_EXCEPTION.to_string()
}
