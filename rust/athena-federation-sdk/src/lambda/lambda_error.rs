use std::error::Error as StdError;
use std::fmt;

use crate::exceptions::{FederationError, JavaThrowable};

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
            .map(|e| e.code.as_str().to_string());
        let mut causes = Vec::new();
        let mut next = error.java_cause();
        while let Some(cause) = next {
            causes.push(LambdaErrorCause {
                error_type: cause.java_class_name(),
                error_message: cause.java_message(),
            });
            next = cause.java_cause();
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
