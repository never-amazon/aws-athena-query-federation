use std::any::Any;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use super::error_details::{ErrorDetails, FederationSourceErrorCode};

pub type Response = Arc<dyn Any + Send + Sync>;
pub type Cause = Arc<dyn StdError + Send + Sync>;

#[derive(Clone)]
pub struct AthenaConnectorException {
    message: String,
    response: Option<Response>,
    cause: Option<Cause>,
    error_details: ErrorDetails,
}

impl AthenaConnectorException {
    pub fn new(message: impl Into<String>, error_details: ErrorDetails) -> Self {
        AthenaConnectorException {
            message: message.into(),
            response: None,
            cause: None,
            error_details,
        }
    }

    pub fn with_response(response: Response, message: impl Into<String>, error_details: ErrorDetails) -> Self {
        AthenaConnectorException {
            message: message.into(),
            response: Some(response),
            cause: None,
            error_details,
        }
    }

    pub fn with_cause(message: impl Into<String>, cause: Cause, error_details: ErrorDetails) -> Self {
        AthenaConnectorException {
            message: message.into(),
            response: None,
            cause: Some(cause),
            error_details,
        }
    }

    pub fn with_response_and_cause(
        response: Response,
        message: impl Into<String>,
        cause: Cause,
        error_details: ErrorDetails,
    ) -> Self {
        AthenaConnectorException {
            message: message.into(),
            response: Some(response),
            cause: Some(cause),
            error_details,
        }
    }

    pub fn of(code: FederationSourceErrorCode, message: impl Into<String>) -> Self {
        AthenaConnectorException::new(message, ErrorDetails::of(code))
    }

    pub fn get_message(&self) -> &str {
        &self.message
    }

    pub fn get_response(&self) -> Option<&Response> {
        self.response.as_ref()
    }

    pub fn get_cause(&self) -> Option<&Cause> {
        self.cause.as_ref()
    }

    pub fn get_error_details(&self) -> &ErrorDetails {
        &self.error_details
    }

    pub fn error_code(&self) -> Option<FederationSourceErrorCode> {
        self.error_details
            .error_code()
            .map(FederationSourceErrorCode::from_value)
    }
}

impl fmt::Debug for AthenaConnectorException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AthenaConnectorException")
            .field("message", &self.message)
            .field("has_response", &self.response.is_some())
            .field("cause", &self.cause.as_ref().map(|c| c.to_string()))
            .field("error_details", &self.error_details)
            .finish()
    }
}

impl fmt::Display for AthenaConnectorException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for AthenaConnectorException {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause.as_ref().map(|c| c.as_ref() as &(dyn StdError + 'static))
    }
}
