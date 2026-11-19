use std::error::Error;
use std::fmt;

use athena_federation_sdk::exceptions::{class_cast_message, JavaExceptionKind};
use athena_federation_sdk::FederationError;

#[derive(Debug, thiserror::Error)]
pub enum SubstraitError {
    #[error("{0}")]
    Runtime(String),
    #[error("{0}")]
    IllegalArgument(String),
    #[error("{0}")]
    IllegalState(String),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    IndexOutOfBounds(String),
    #[error("{0}")]
    NullPointer(String),
    #[error("{0}")]
    NumberFormat(String),
    #[error("{}", class_cast_message(from, to))]
    ClassCast {
        from: &'static str,
        to: &'static str,
    },
    #[error("{message}")]
    Wrapped {
        message: &'static str,
        #[source]
        cause: Box<SubstraitError>,
    },
    #[error("{0}")]
    Base64(#[from] base64::DecodeError),
    #[error("{0}")]
    Decode(#[from] prost::DecodeError),
}

pub(crate) const REX_CALL: &str = "org.apache.calcite.rex.RexCall";
pub(crate) const REX_INPUT_REF: &str = "org.apache.calcite.rex.RexInputRef";
pub(crate) const REX_SLOT: &str = "org.apache.calcite.rex.RexSlot";

pub type Result<T> = std::result::Result<T, SubstraitError>;

impl SubstraitError {
    pub fn wrap(message: &'static str, cause: SubstraitError) -> SubstraitError {
        SubstraitError::Wrapped {
            message,
            cause: Box::new(cause),
        }
    }

    pub fn java_kind(&self) -> JavaExceptionKind {
        match self {
            SubstraitError::Runtime(_)
            | SubstraitError::Wrapped { .. }
            | SubstraitError::Decode(_) => JavaExceptionKind::Runtime,
            SubstraitError::IllegalArgument(_) | SubstraitError::Base64(_) => {
                JavaExceptionKind::IllegalArgument
            }
            SubstraitError::IllegalState(_) => JavaExceptionKind::IllegalState,
            SubstraitError::Unsupported(_) => JavaExceptionKind::UnsupportedOperation,
            SubstraitError::IndexOutOfBounds(_) => JavaExceptionKind::IndexOutOfBounds,
            SubstraitError::NullPointer(_) => JavaExceptionKind::NullPointer,
            SubstraitError::NumberFormat(_) => JavaExceptionKind::NumberFormat,
            SubstraitError::ClassCast { .. } => {
                JavaExceptionKind::Unchecked("java.lang.ClassCastException")
            }
        }
    }
}

#[derive(Debug)]
struct CausedSubstraitError {
    message: &'static str,
    cause: FederationError,
}

impl fmt::Display for CausedSubstraitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl Error for CausedSubstraitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.cause)
    }
}

impl From<SubstraitError> for FederationError {
    fn from(error: SubstraitError) -> Self {
        let kind = error.java_kind();
        match error {
            SubstraitError::Wrapped { message, cause } => FederationError::wrapped(
                kind,
                CausedSubstraitError {
                    message,
                    cause: FederationError::from(*cause),
                },
            ),
            other => FederationError::wrapped(kind, other),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::{FederationError, JavaExceptionKind, SubstraitError};

    #[test]
    fn substrait_error_keeps_kind_message_and_source() {
        let cause = SubstraitError::IllegalArgument("bad input".into());
        let cause_text = cause.to_string();
        let error = SubstraitError::wrap("Failed to parse Substrait plan", cause);
        let error_text = error.to_string();
        let federation = FederationError::from(error);
        assert_eq!(federation.to_string(), "Failed to parse Substrait plan");
        let FederationError::Wrapped { kind, source } = &federation else {
            panic!("expected a wrapped error, got {federation:?}");
        };
        assert_eq!(*kind, JavaExceptionKind::Runtime);
        assert_eq!(source.to_string(), error_text);
        let inner = federation
            .source()
            .and_then(|s| s.source())
            .and_then(|s| s.downcast_ref::<FederationError>());
        assert_eq!(inner.map(ToString::to_string), Some(cause_text));
        assert_eq!(
            inner.map(FederationError::java_class_name).as_deref(),
            Some("java.lang.IllegalArgumentException")
        );
    }
}
