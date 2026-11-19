use std::error::Error as StdError;
use std::sync::Arc;

use arrow::error::ArrowError;
use thiserror::Error;

use super::athena_connector_exception::AthenaConnectorException;
use super::error_details::FederationSourceErrorCode;
use super::federation_throttle_exception::FederationThrottleException;
use super::java_throwable::JavaThrowable;

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

pub type Result<T> = std::result::Result<T, FederationError>;

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
