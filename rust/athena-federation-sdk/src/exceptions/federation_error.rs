use std::error::Error as StdError;
use std::sync::Arc;

use arrow::error::ArrowError;
use thiserror::Error;

use super::athena_connector_exception::AthenaConnectorException;
use super::error_details::{ErrorDetails, FederationSourceErrorCode};
use super::federation_throttle_exception::FederationThrottleException;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JavaExceptionKind {
    Runtime,
    IllegalArgument,
    IllegalState,
    UnsupportedOperation,
    IndexOutOfBounds,
    NullPointer,
    NumberFormat,
    Unchecked(&'static str),
    Checked(&'static str),
}

impl JavaExceptionKind {
    pub fn java_class_name(&self) -> &'static str {
        match self {
            JavaExceptionKind::Runtime => RUNTIME_EXCEPTION,
            JavaExceptionKind::IllegalArgument => "java.lang.IllegalArgumentException",
            JavaExceptionKind::IllegalState => "java.lang.IllegalStateException",
            JavaExceptionKind::UnsupportedOperation => "java.lang.UnsupportedOperationException",
            JavaExceptionKind::IndexOutOfBounds => "java.lang.IndexOutOfBoundsException",
            JavaExceptionKind::NullPointer => "java.lang.NullPointerException",
            JavaExceptionKind::NumberFormat => "java.lang.NumberFormatException",
            JavaExceptionKind::Unchecked(name) | JavaExceptionKind::Checked(name) => name,
        }
    }

    pub fn is_runtime_exception(&self) -> bool {
        !matches!(self, JavaExceptionKind::Checked(_))
    }
}

const RUNTIME_EXCEPTION: &str = "java.lang.RuntimeException";
const IO_EXCEPTION: &str = "java.io.IOException";
const AWS_SERVICE_EXCEPTION: &str = "software.amazon.awssdk.awscore.exception.AwsServiceException";

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
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    IoError(#[source] Arc<std::io::Error>),
    #[error("{0}")]
    Runtime(String),
    #[error("{}", .0.get_message().unwrap_or_default())]
    AwsService(crate::security::AwsServiceError),
    #[error("{source}")]
    Wrapped {
        kind: JavaExceptionKind,
        #[source]
        source: Arc<dyn StdError + Send + Sync>,
    },
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

#[derive(Debug, Clone, Error)]
#[error("{message}")]
struct CausedRuntimeException {
    message: String,
    #[source]
    cause: Arc<FederationError>,
}

impl FederationError {
    pub fn connector(
        code: FederationSourceErrorCode,
        message: impl Into<String>,
    ) -> FederationError {
        FederationError::AthenaConnector(AthenaConnectorException::new(
            message,
            ErrorDetails::of(code),
        ))
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

    pub fn wrapped(
        kind: JavaExceptionKind,
        source: impl StdError + Send + Sync + 'static,
    ) -> FederationError {
        FederationError::Wrapped {
            kind,
            source: Arc::new(source),
        }
    }

    pub fn runtime_with_cause(
        message: impl Into<String>,
        cause: FederationError,
    ) -> FederationError {
        FederationError::wrapped(
            JavaExceptionKind::Runtime,
            CausedRuntimeException {
                message: message.into(),
                cause: Arc::new(cause),
            },
        )
    }

    pub fn java_message(&self) -> Option<String> {
        match self {
            FederationError::NullPointer(None) => None,
            other => Some(other.to_string()),
        }
    }

    pub fn as_connector_exception(&self) -> Option<&AthenaConnectorException> {
        match self {
            FederationError::AthenaConnector(e) => Some(e),
            _ => None,
        }
    }

    pub fn error_code(&self) -> Option<FederationSourceErrorCode> {
        self.as_connector_exception()
            .and_then(super::athena_connector_exception::AthenaConnectorException::error_code)
    }

    pub fn is_runtime_exception(&self) -> bool {
        match self {
            FederationError::Io(_) | FederationError::IoError(_) => false,
            FederationError::Wrapped { kind, .. } => kind.is_runtime_exception(),
            _ => true,
        }
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

    pub fn java_class_name(&self) -> String {
        let name = match self {
            FederationError::AthenaConnector(_) => {
                "com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException"
            }
            FederationError::Throttle(_) => {
                "com.amazonaws.athena.connector.lambda.exceptions.FederationThrottleException"
            }
            FederationError::IllegalArgument(_) => {
                JavaExceptionKind::IllegalArgument.java_class_name()
            }
            FederationError::IllegalState(_) => JavaExceptionKind::IllegalState.java_class_name(),
            FederationError::UnsupportedOperation(_) => {
                JavaExceptionKind::UnsupportedOperation.java_class_name()
            }
            FederationError::ClassCast(_) => "java.lang.ClassCastException",
            FederationError::NullPointer(_) => JavaExceptionKind::NullPointer.java_class_name(),
            FederationError::Arithmetic(_) => "java.lang.ArithmeticException",
            FederationError::IndexOutOfBounds(_) => {
                JavaExceptionKind::IndexOutOfBounds.java_class_name()
            }
            FederationError::NumberFormat(_) => JavaExceptionKind::NumberFormat.java_class_name(),
            FederationError::Io(_) | FederationError::IoError(_) => IO_EXCEPTION,
            FederationError::Arrow(_)
            | FederationError::Runtime(_)
            | FederationError::RuntimeWrapper { .. } => RUNTIME_EXCEPTION,
            FederationError::AwsService(e) => {
                return e
                    .java_class_name()
                    .unwrap_or_else(|| AWS_SERVICE_EXCEPTION.to_string())
            }
            FederationError::Wrapped { kind, .. } => kind.java_class_name(),
        };
        name.to_string()
    }
}

impl From<ArrowError> for FederationError {
    fn from(e: ArrowError) -> Self {
        FederationError::Arrow(Arc::new(e))
    }
}

impl From<std::io::Error> for FederationError {
    fn from(e: std::io::Error) -> Self {
        FederationError::IoError(Arc::new(e))
    }
}
