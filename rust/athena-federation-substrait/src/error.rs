use std::error::Error;
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Runtime,
    IllegalArgument,
    IllegalState,
    UnsupportedOperation,
    IndexOutOfBounds,
    NullPointer,
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

impl From<SubstraitError> for athena_federation_sdk::FederationError {
    fn from(error: SubstraitError) -> Self {
        let message = error.message;
        match error.kind {
            ErrorKind::Runtime => athena_federation_sdk::FederationError::Runtime(message),
            ErrorKind::IllegalArgument => {
                athena_federation_sdk::FederationError::IllegalArgument(message)
            }
            ErrorKind::IllegalState => {
                athena_federation_sdk::FederationError::IllegalState(message)
            }
            ErrorKind::UnsupportedOperation => {
                athena_federation_sdk::FederationError::UnsupportedOperation(message)
            }
            ErrorKind::IndexOutOfBounds => {
                athena_federation_sdk::FederationError::IndexOutOfBounds(message)
            }
            ErrorKind::NullPointer => athena_federation_sdk::FederationError::NullPointer(message),
        }
    }
}
