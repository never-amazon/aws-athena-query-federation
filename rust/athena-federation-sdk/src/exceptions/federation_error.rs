use arrow::error::ArrowError;
use thiserror::Error;

use super::athena_connector_exception::AthenaConnectorException;
use super::error_details::{ErrorDetails, FederationSourceErrorCode};
use super::federation_throttle_exception::FederationThrottleException;

#[derive(Debug, Error, Clone)]
pub enum FederationError {
    #[error(transparent)]
    AthenaConnector(#[from] AthenaConnectorException),
    #[error(transparent)]
    Throttle(#[from] FederationThrottleException),
    #[error("{0}")]
    IllegalArgument(String),
    #[error("{0}")]
    IllegalState(String),
    #[error("{0}")]
    UnsupportedOperation(String),
    #[error("{0}")]
    ClassCast(String),
    #[error("{0}")]
    NullPointer(String),
    #[error("{0}")]
    Arithmetic(String),
    #[error("{0}")]
    IndexOutOfBounds(String),
    #[error("{0}")]
    Arrow(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Runtime(String),
}

pub type Result<T> = std::result::Result<T, FederationError>;

impl FederationError {
    pub fn connector(code: FederationSourceErrorCode, message: impl Into<String>) -> FederationError {
        FederationError::AthenaConnector(AthenaConnectorException::new(message, ErrorDetails::of(code)))
    }

    pub fn connector_with_details(
        code: FederationSourceErrorCode,
        message: impl Into<String>,
        details_message: impl Into<String>,
    ) -> FederationError {
        FederationError::AthenaConnector(AthenaConnectorException::new(
            message,
            ErrorDetails::of_with_message(code, details_message),
        ))
    }

    pub fn invalid_input(message: impl Into<String>) -> FederationError {
        FederationError::connector(FederationSourceErrorCode::InvalidInputException, message)
    }

    pub fn not_supported(message: impl Into<String>) -> FederationError {
        FederationError::connector(FederationSourceErrorCode::OperationNotSupportedException, message)
    }

    pub fn internal(message: impl Into<String>) -> FederationError {
        FederationError::connector(FederationSourceErrorCode::InternalServiceException, message)
    }

    pub fn illegal_argument(message: impl Into<String>) -> FederationError {
        FederationError::IllegalArgument(message.into())
    }

    pub fn illegal_state(message: impl Into<String>) -> FederationError {
        FederationError::IllegalState(message.into())
    }

    pub fn as_connector_exception(&self) -> Option<&AthenaConnectorException> {
        match self {
            FederationError::AthenaConnector(e) => Some(e),
            _ => None,
        }
    }

    pub fn error_code(&self) -> Option<FederationSourceErrorCode> {
        self.as_connector_exception().and_then(|e| e.error_code())
    }

    pub fn is_runtime_exception(&self) -> bool {
        !matches!(self, FederationError::Io(_))
    }
}

impl From<ArrowError> for FederationError {
    fn from(e: ArrowError) -> Self {
        FederationError::Arrow(e.to_string())
    }
}

impl From<std::io::Error> for FederationError {
    fn from(e: std::io::Error) -> Self {
        FederationError::Io(e.to_string())
    }
}
