use std::fmt;

use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::error::display::DisplayErrorContext;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;

use crate::exceptions::{AthenaConnectorException, ErrorDetails, FederationError, FederationSourceErrorCode};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AwsServiceError {
    error_code: Option<String>,
    message: String,
    status_code: Option<u16>,
}

impl AwsServiceError {
    pub const IO_ERROR_CODE: &'static str = "IOException";

    pub fn new(message: impl Into<String>) -> AwsServiceError {
        AwsServiceError {
            error_code: None,
            message: message.into(),
            status_code: None,
        }
    }

    pub fn with_code(error_code: impl Into<String>, message: impl Into<String>) -> AwsServiceError {
        AwsServiceError {
            error_code: Some(error_code.into()),
            message: message.into(),
            status_code: None,
        }
    }

    pub fn io(message: impl Into<String>) -> AwsServiceError {
        AwsServiceError::with_code(Self::IO_ERROR_CODE, message)
    }

    pub fn with_status_code(mut self, status_code: u16) -> AwsServiceError {
        self.status_code = Some(status_code);
        self
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    pub fn is_error_code(&self, code: &str) -> bool {
        self.error_code.as_deref() == Some(code)
    }

    pub fn from_sdk_error<E>(err: &SdkError<E, HttpResponse>) -> AwsServiceError
    where
        E: ProvideErrorMetadata + std::error::Error + 'static,
    {
        let status_code = err.raw_response().map(|r| r.status().as_u16());
        let (code, message) = match err.as_service_error() {
            Some(service) => (
                service.code().map(str::to_string),
                service
                    .message()
                    .map(str::to_string)
                    .unwrap_or_else(|| service.to_string()),
            ),
            None => (None, DisplayErrorContext(err).to_string()),
        };
        AwsServiceError {
            error_code: code,
            message,
            status_code,
        }
    }
}

impl fmt::Display for AwsServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AwsServiceError {}

impl From<AwsServiceError> for FederationError {
    fn from(e: AwsServiceError) -> Self {
        if e.is_error_code(AwsServiceError::IO_ERROR_CODE) {
            let message = e.message.clone();
            return FederationError::AthenaConnector(AthenaConnectorException::with_cause(
                message,
                std::sync::Arc::new(e),
                ErrorDetails::of(FederationSourceErrorCode::InternalServiceException),
            ));
        }
        FederationError::Runtime(e.message)
    }
}

impl From<FederationError> for AwsServiceError {
    fn from(e: FederationError) -> Self {
        AwsServiceError {
            error_code: e.error_code().map(|c| c.as_str().to_string()),
            message: e.to_string(),
            status_code: None,
        }
    }
}
