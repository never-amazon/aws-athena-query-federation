use std::error::Error;
use std::fmt;
use std::sync::Arc;

use athena_federation_sdk::exceptions::{class_cast_message, JavaExceptionKind};
use athena_federation_sdk::FederationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Runtime,
    IllegalArgument,
    IllegalState,
    UnsupportedOperation,
    IndexOutOfBounds,
    NullPointer,
    ClassCast,
}

#[derive(Debug, Clone)]
pub struct SubstraitError {
    kind: ErrorKind,
    message: String,
    cause: Option<Box<SubstraitError>>,
    source: Option<Arc<dyn Error + Send + Sync>>,
}

pub type Result<T> = std::result::Result<T, SubstraitError>;

impl SubstraitError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> SubstraitError {
        SubstraitError {
            kind,
            message: message.into(),
            cause: None,
            source: None,
        }
    }

    pub fn from_source(
        kind: ErrorKind,
        source: impl Error + Send + Sync + 'static,
    ) -> SubstraitError {
        SubstraitError {
            kind,
            message: source.to_string(),
            cause: None,
            source: Some(Arc::new(source)),
        }
    }

    pub fn runtime(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::Runtime, message)
    }

    pub fn illegal_argument(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::IllegalArgument, message)
    }

    pub fn illegal_state(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::IllegalState, message)
    }

    pub fn unsupported(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::UnsupportedOperation, message)
    }

    pub fn index_out_of_bounds(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::IndexOutOfBounds, message)
    }

    pub fn class_cast(from: &str, to: &str) -> SubstraitError {
        SubstraitError::new(ErrorKind::ClassCast, class_cast_message(from, to))
    }

    pub fn null_pointer(message: impl Into<String>) -> SubstraitError {
        SubstraitError::new(ErrorKind::NullPointer, message)
    }

    #[must_use]
    pub fn with_cause(mut self, cause: SubstraitError) -> SubstraitError {
        self.cause = Some(Box::new(cause));
        self
    }

    pub fn wrap(message: impl Into<String>, cause: SubstraitError) -> SubstraitError {
        SubstraitError::runtime(message).with_cause(cause)
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn cause(&self) -> Option<&SubstraitError> {
        self.cause.as_deref()
    }

    pub fn root_cause(&self) -> &SubstraitError {
        let mut current = self;
        while let Some(next) = current.cause.as_deref() {
            current = next;
        }
        current
    }
}

impl fmt::Display for SubstraitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl PartialEq for SubstraitError {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.message == other.message && self.cause == other.cause
    }
}

impl Error for SubstraitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match (&self.cause, &self.source) {
            (Some(c), _) => Some(c.as_ref()),
            (None, Some(s)) => Some(s.as_ref()),
            (None, None) => None,
        }
    }
}

impl From<ErrorKind> for JavaExceptionKind {
    fn from(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::Runtime => JavaExceptionKind::Runtime,
            ErrorKind::IllegalArgument => JavaExceptionKind::IllegalArgument,
            ErrorKind::IllegalState => JavaExceptionKind::IllegalState,
            ErrorKind::UnsupportedOperation => JavaExceptionKind::UnsupportedOperation,
            ErrorKind::IndexOutOfBounds => JavaExceptionKind::IndexOutOfBounds,
            ErrorKind::NullPointer => JavaExceptionKind::NullPointer,
            ErrorKind::ClassCast => JavaExceptionKind::Unchecked("java.lang.ClassCastException"),
        }
    }
}

impl From<SubstraitError> for FederationError {
    fn from(error: SubstraitError) -> Self {
        FederationError::wrapped(error.kind.into(), error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::{FederationError, JavaExceptionKind, SubstraitError};

    #[test]
    fn substrait_error_keeps_kind_message_and_source() {
        let cause = SubstraitError::illegal_argument("bad input");
        let error = SubstraitError::wrap("Failed to parse Substrait plan", cause.clone());
        let federation = FederationError::from(error.clone());
        assert_eq!(federation.to_string(), "Failed to parse Substrait plan");
        let FederationError::Wrapped { kind, source } = &federation else {
            panic!("expected a wrapped error, got {federation:?}");
        };
        assert_eq!(*kind, JavaExceptionKind::Runtime);
        assert_eq!(source.downcast_ref::<SubstraitError>(), Some(&error));
        let inner = federation.source().and_then(|s| s.source());
        assert_eq!(inner.map(ToString::to_string), Some(cause.to_string()));
    }
}
