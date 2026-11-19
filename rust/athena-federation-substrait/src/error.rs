use athena_federation_sdk::exceptions::{class_cast_message, JavaCause, JavaThrowable};
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
}

impl JavaThrowable for SubstraitError {
    fn java_class_name(&self) -> String {
        let name = match self {
            SubstraitError::Runtime(_)
            | SubstraitError::Wrapped { .. }
            | SubstraitError::Decode(_) => "java.lang.RuntimeException",
            SubstraitError::IllegalArgument(_) | SubstraitError::Base64(_) => {
                "java.lang.IllegalArgumentException"
            }
            SubstraitError::IllegalState(_) => "java.lang.IllegalStateException",
            SubstraitError::Unsupported(_) => "java.lang.UnsupportedOperationException",
            SubstraitError::IndexOutOfBounds(_) => "java.lang.IndexOutOfBoundsException",
            SubstraitError::NullPointer(_) => "java.lang.NullPointerException",
            SubstraitError::NumberFormat(_) => "java.lang.NumberFormatException",
            SubstraitError::ClassCast { .. } => "java.lang.ClassCastException",
        };
        name.to_string()
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        match self {
            SubstraitError::Wrapped { cause, .. } => Some(JavaCause::Throwable(cause.as_ref())),
            SubstraitError::Base64(e) => Some(JavaCause::foreign(e)),
            SubstraitError::Decode(e) => Some(JavaCause::foreign(e)),
            _ => None,
        }
    }
}

impl From<SubstraitError> for FederationError {
    fn from(error: SubstraitError) -> Self {
        FederationError::external(error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::{FederationError, JavaThrowable, SubstraitError};

    #[test]
    fn substrait_error_keeps_kind_message_and_source() {
        let cause = SubstraitError::IllegalArgument("bad input".into());
        let cause_text = cause.to_string();
        let error = SubstraitError::wrap("Failed to parse Substrait plan", cause);
        let error_text = error.to_string();
        let federation = FederationError::from(error);
        assert_eq!(federation.to_string(), "Failed to parse Substrait plan");
        assert_eq!(federation.java_class_name(), "java.lang.RuntimeException");
        let source = federation.source().expect("source");
        assert_eq!(source.to_string(), error_text);
        let inner = source
            .source()
            .and_then(|s| s.downcast_ref::<Box<SubstraitError>>())
            .map(AsRef::as_ref);
        assert_eq!(inner.map(ToString::to_string), Some(cause_text));
        assert_eq!(
            inner.map(SubstraitError::java_class_name).as_deref(),
            Some("java.lang.IllegalArgumentException")
        );
    }
}
