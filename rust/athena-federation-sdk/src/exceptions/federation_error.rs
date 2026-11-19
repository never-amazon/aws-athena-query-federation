use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use arrow::error::ArrowError;
use thiserror::Error;

use crate::exceptions::JavaThrowable;

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
    UnexpectedToken(String),
    #[error("{0}")]
    UnsupportedOperation(String),
    #[error("{0}")]
    ClassCast(String),
    #[error("{}", .0.as_deref().unwrap_or("null"))]
    NullPointer(Option<String>),
    #[error("{0}")]
    Arithmetic(String),
    #[error("{0}")]
    IndexOutOfBounds(String),
    #[error("{0}")]
    NumberFormat(String),
    #[error("{0}")]
    Arrow(#[source] Arc<ArrowError>),
    #[error("{message}")]
    Io {
        message: String,
        #[source]
        source: Option<Arc<std::io::Error>>,
    },
    #[error("{message}")]
    Runtime {
        message: String,
        #[source]
        source: Option<Arc<dyn StdError + Send + Sync>>,
    },
    #[error("{}", .0.get_message().unwrap_or_default())]
    AwsService(crate::security::AwsServiceError),
    #[error("{message}")]
    RuntimeWithCause {
        message: String,
        #[source]
        cause: Arc<FederationError>,
    },
    #[error("{0}")]
    Other(#[source] Arc<dyn StdError + Send + Sync>),
    #[error("{0}")]
    External(#[source] Arc<dyn JavaThrowable>),
    #[error("{}", java_to_string(source))]
    RuntimeWrapper {
        #[source]
        source: Arc<FederationError>,
    },
}

fn java_to_string(error: &FederationError) -> String {
    match error.java_message() {
        Some(message) => format!("{}: {message}", error.java_class_name()),
        None => error.java_class_name(),
    }
}

pub type Result<T, E = FederationError> = std::result::Result<T, E>;

impl FederationError {
    pub fn connector(
        code: FederationSourceErrorCode,
        message: impl Into<String>,
    ) -> FederationError {
        FederationError::AthenaConnector(AthenaConnectorException::new(code, message))
    }

    pub fn connector_with_details(
        code: FederationSourceErrorCode,
        message: impl Into<String>,
        details_message: impl Into<String>,
    ) -> FederationError {
        FederationError::AthenaConnector(
            AthenaConnectorException::new(code, message).with_details_message(details_message),
        )
    }

    pub fn invalid_input(message: impl Into<String>) -> FederationError {
        FederationError::connector(FederationSourceErrorCode::InvalidInputException, message)
    }

    pub fn not_supported(message: impl Into<String>) -> FederationError {
        FederationError::connector(
            FederationSourceErrorCode::OperationNotSupportedException,
            message,
        )
    }

    pub fn internal(message: impl Into<String>) -> FederationError {
        FederationError::connector(FederationSourceErrorCode::InternalServiceException, message)
    }

    pub fn illegal_argument(message: impl Into<String>) -> FederationError {
        FederationError::IllegalArgument(message.into())
    }

    pub fn class_cast(from: &str, to: &str) -> FederationError {
        FederationError::ClassCast(super::class_cast_message(from, to))
    }

    pub fn null_pointer(message: impl Into<String>) -> FederationError {
        FederationError::NullPointer(Some(message.into()))
    }

    pub fn illegal_state(message: impl Into<String>) -> FederationError {
        FederationError::IllegalState(message.into())
    }

    pub fn io(message: impl Into<String>) -> FederationError {
        FederationError::Io {
            message: message.into(),
            source: None,
        }
    }

    pub fn runtime(message: impl Into<String>) -> FederationError {
        FederationError::Runtime {
            message: message.into(),
            source: None,
        }
    }

    pub fn runtime_from(
        message: impl Into<String>,
        source: impl StdError + Send + Sync + 'static,
    ) -> FederationError {
        FederationError::Runtime {
            message: message.into(),
            source: Some(Arc::new(source)),
        }
    }

    pub fn external(error: impl JavaThrowable) -> FederationError {
        FederationError::External(Arc::new(error))
    }

    pub fn other(error: impl StdError + Send + Sync + 'static) -> FederationError {
        FederationError::Other(Arc::new(error))
    }

    pub fn runtime_with_cause(
        message: impl Into<String>,
        cause: FederationError,
    ) -> FederationError {
        FederationError::RuntimeWithCause {
            message: message.into(),
            cause: Arc::new(cause),
        }
    }

    pub fn downcast_external<T: JavaThrowable>(&self) -> Option<&T> {
        match self {
            FederationError::External(error) => {
                let error: &(dyn StdError + 'static) = error.as_ref();
                error.downcast_ref::<T>()
            }
            _ => None,
        }
    }

    pub fn as_connector_exception(&self) -> Option<&AthenaConnectorException> {
        match self {
            FederationError::AthenaConnector(e) => Some(e),
            _ => None,
        }
    }

    pub fn error_code(&self) -> Option<FederationSourceErrorCode> {
        self.as_connector_exception().map(|e| e.code)
    }

    #[must_use]
    pub fn into_runtime_exception(self) -> FederationError {
        if self.is_runtime_exception() {
            self
        } else {
            FederationError::RuntimeWrapper {
                source: Arc::new(self),
            }
        }
    }
}

impl From<ArrowError> for FederationError {
    fn from(e: ArrowError) -> Self {
        FederationError::Arrow(Arc::new(e))
    }
}

impl From<std::io::Error> for FederationError {
    fn from(e: std::io::Error) -> Self {
        FederationError::Io {
            message: e.to_string(),
            source: Some(Arc::new(e)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FederationSourceErrorCode {
    AccessDeniedException,
    EntityNotFoundException,
    InvalidCredentialsException,
    InvalidInputException,
    InvalidResponseException,
    OperationTimeoutException,
    OperationNotSupportedException,
    InternalServiceException,
    PartialFailureException,
    ThrottlingException,
    UnknownToSdkVersion,
}

impl FederationSourceErrorCode {
    pub fn from_http_status(status: u16) -> FederationSourceErrorCode {
        match status {
            400 | 409 => FederationSourceErrorCode::InvalidInputException,
            401 => FederationSourceErrorCode::InvalidCredentialsException,
            403 => FederationSourceErrorCode::AccessDeniedException,
            404 => FederationSourceErrorCode::EntityNotFoundException,
            408 | 504 => FederationSourceErrorCode::OperationTimeoutException,
            429 => FederationSourceErrorCode::ThrottlingException,
            500 | 502 | 503 => FederationSourceErrorCode::InternalServiceException,
            501 => FederationSourceErrorCode::OperationNotSupportedException,
            other => {
                tracing::warn!("Unknown error code: {}", other);
                FederationSourceErrorCode::InvalidInputException
            }
        }
    }

    pub fn as_str(&self) -> &'static str {
        use FederationSourceErrorCode::{
            AccessDeniedException, EntityNotFoundException, InternalServiceException,
            InvalidCredentialsException, InvalidInputException, InvalidResponseException,
            OperationNotSupportedException, OperationTimeoutException, PartialFailureException,
            ThrottlingException, UnknownToSdkVersion,
        };
        match self {
            AccessDeniedException => "AccessDeniedException",
            EntityNotFoundException => "EntityNotFoundException",
            InvalidCredentialsException => "InvalidCredentialsException",
            InvalidInputException => "InvalidInputException",
            InvalidResponseException => "InvalidResponseException",
            OperationTimeoutException => "OperationTimeoutException",
            OperationNotSupportedException => "OperationNotSupportedException",
            InternalServiceException => "InternalServiceException",
            PartialFailureException => "PartialFailureException",
            ThrottlingException => "ThrottlingException",
            UnknownToSdkVersion => "UNKNOWN_TO_SDK_VERSION",
        }
    }
}

impl fmt::Display for FederationSourceErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

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

#[derive(Clone, Default)]
pub struct FederationThrottleException {
    pub message: Option<String>,
    pub cause: Option<Arc<dyn JavaThrowable>>,
}

impl FederationThrottleException {
    pub fn new(message: impl Into<String>) -> Self {
        FederationThrottleException {
            message: Some(message.into()),
            cause: None,
        }
    }

    #[must_use]
    pub fn with_cause(mut self, cause: impl JavaThrowable) -> Self {
        self.cause = Some(Arc::new(cause));
        self
    }
}

impl fmt::Debug for FederationThrottleException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FederationThrottleException")
            .field("message", &self.message)
            .field(
                "cause",
                &self.cause.as_ref().map(std::string::ToString::to_string),
            )
            .finish()
    }
}

impl fmt::Display for FederationThrottleException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            self.message
                .as_deref()
                .unwrap_or("FederationThrottleException"),
        )
    }
}

impl std::error::Error for FederationThrottleException {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}
