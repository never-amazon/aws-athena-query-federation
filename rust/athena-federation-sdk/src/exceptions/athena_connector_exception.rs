use std::fmt;
use std::sync::Arc;

use super::error_details::FederationSourceErrorCode;
use super::federation_error::FederationError;
use super::java_throwable::JavaThrowable;

#[derive(Clone, thiserror::Error)]
#[error("{message}")]
pub struct AthenaConnectorException {
    pub message: String,
    pub code: FederationSourceErrorCode,
    pub details_message: Option<String>,
    #[source]
    pub cause: Option<Arc<dyn JavaThrowable>>,
    pub response: Option<Arc<FederationError>>,
}

impl AthenaConnectorException {
    pub fn new(code: FederationSourceErrorCode, message: impl Into<String>) -> Self {
        AthenaConnectorException {
            message: message.into(),
            code,
            details_message: None,
            cause: None,
            response: None,
        }
    }

    #[must_use]
    pub fn with_details_message(mut self, details_message: impl Into<String>) -> Self {
        self.details_message = Some(details_message.into());
        self
    }

    #[must_use]
    pub fn with_cause(mut self, cause: impl JavaThrowable) -> Self {
        self.cause = Some(Arc::new(cause));
        self
    }

    #[must_use]
    pub fn with_response(mut self, response: FederationError) -> Self {
        self.response = Some(Arc::new(response));
        self
    }
}

impl fmt::Debug for AthenaConnectorException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AthenaConnectorException")
            .field("message", &self.message)
            .field("code", &self.code)
            .field("details_message", &self.details_message)
            .field(
                "cause",
                &self.cause.as_ref().map(std::string::ToString::to_string),
            )
            .field("has_response", &self.response.is_some())
            .finish()
    }
}
