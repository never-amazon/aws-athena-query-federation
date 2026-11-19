use std::error::Error as StdError;
use std::sync::Arc;

use super::athena_connector_exception::AthenaConnectorException;
use super::federation_error::FederationError;
use crate::security::AwsServiceError;

const RUNTIME_EXCEPTION: &str = "java.lang.RuntimeException";
const IO_EXCEPTION: &str = "java.io.IOException";
const ATHENA_CONNECTOR_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException";
const FEDERATION_THROTTLE_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.FederationThrottleException";
const AWS_SERVICE_EXCEPTION: &str = "software.amazon.awssdk.awscore.exception.AwsServiceException";

pub trait JavaThrowable: StdError + Send + Sync + 'static {
    fn java_class_name(&self) -> String;

    fn java_message(&self) -> Option<String> {
        Some(self.to_string())
    }

    fn java_cause(&self) -> Option<JavaCause<'_>>;

    fn is_runtime_exception(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy)]
pub enum JavaCause<'a> {
    Throwable(&'a dyn JavaThrowable),
    Foreign(&'a (dyn StdError + 'static)),
}

impl<'a> JavaCause<'a> {
    pub fn foreign(error: &'a (dyn StdError + 'static)) -> JavaCause<'a> {
        let federation = error.downcast_ref::<FederationError>().or_else(|| {
            error
                .downcast_ref::<Arc<FederationError>>()
                .map(AsRef::as_ref)
        });
        match federation {
            Some(federation) => JavaCause::Throwable(federation),
            None => JavaCause::Foreign(error),
        }
    }

    pub fn java_class_name(self) -> String {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_class_name(),
            JavaCause::Foreign(error) => {
                if error.is::<AthenaConnectorException>() {
                    ATHENA_CONNECTOR_EXCEPTION.to_string()
                } else if let Some(aws) = error.downcast_ref::<AwsServiceError>() {
                    aws_class_name(aws)
                } else if error.is::<std::io::Error>() {
                    IO_EXCEPTION.to_string()
                } else {
                    RUNTIME_EXCEPTION.to_string()
                }
            }
        }
    }

    pub fn java_message(self) -> Option<String> {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_message(),
            JavaCause::Foreign(error) => Some(error.to_string()),
        }
    }

    pub fn java_cause(self) -> Option<JavaCause<'a>> {
        match self {
            JavaCause::Throwable(throwable) => throwable.java_cause(),
            JavaCause::Foreign(error) => error.source().map(JavaCause::foreign),
        }
    }
}

fn foreign_source(error: &dyn StdError) -> Option<JavaCause<'_>> {
    error.source().map(JavaCause::foreign)
}

impl JavaThrowable for FederationError {
    fn java_class_name(&self) -> String {
        let name = match self {
            FederationError::AthenaConnector(_) => ATHENA_CONNECTOR_EXCEPTION,
            FederationError::Throttle(_) => FEDERATION_THROTTLE_EXCEPTION,
            FederationError::IllegalArgument(_) => "java.lang.IllegalArgumentException",
            FederationError::IllegalState(_) => "java.lang.IllegalStateException",
            FederationError::UnsupportedOperation(_) => "java.lang.UnsupportedOperationException",
            FederationError::ClassCast(_) => "java.lang.ClassCastException",
            FederationError::NullPointer(_) => "java.lang.NullPointerException",
            FederationError::Arithmetic(_) => "java.lang.ArithmeticException",
            FederationError::IndexOutOfBounds(_) => "java.lang.IndexOutOfBoundsException",
            FederationError::NumberFormat(_) => "java.lang.NumberFormatException",
            FederationError::Io(_) | FederationError::IoError(_) => IO_EXCEPTION,
            FederationError::Arrow(_)
            | FederationError::Runtime(_)
            | FederationError::RuntimeWithCause { .. }
            | FederationError::Other(_)
            | FederationError::RuntimeWrapper { .. } => RUNTIME_EXCEPTION,
            FederationError::AwsService(e) => return aws_class_name(e),
            FederationError::External(e) => return e.java_class_name(),
        };
        name.to_string()
    }

    fn java_message(&self) -> Option<String> {
        match self {
            FederationError::NullPointer(None) => None,
            other => Some(other.to_string()),
        }
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        match self {
            FederationError::RuntimeWrapper { source } => {
                Some(JavaCause::Throwable(source.as_ref()))
            }
            FederationError::RuntimeWithCause { cause, .. } => {
                Some(JavaCause::Throwable(cause.as_ref()))
            }
            FederationError::External(e) => e.java_cause(),
            FederationError::Other(e) => foreign_source(e.as_ref()),
            FederationError::IoError(e) => foreign_source(e.as_ref()),
            FederationError::Arrow(e) => foreign_source(e.as_ref()),
            FederationError::AthenaConnector(e) => foreign_source(e),
            FederationError::Throttle(e) => foreign_source(e),
            _ => None,
        }
    }

    fn is_runtime_exception(&self) -> bool {
        match self {
            FederationError::Io(_) | FederationError::IoError(_) => false,
            FederationError::External(e) => e.is_runtime_exception(),
            _ => true,
        }
    }
}

fn aws_class_name(error: &AwsServiceError) -> String {
    error
        .java_class_name()
        .unwrap_or_else(|| AWS_SERVICE_EXCEPTION.to_string())
}
