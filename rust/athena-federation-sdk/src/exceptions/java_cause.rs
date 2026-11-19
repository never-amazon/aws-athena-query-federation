use std::error::Error as StdError;
use std::sync::Arc;

use super::athena_connector_exception::AthenaConnectorException;
use super::federation_error::FederationError;
use crate::security::AwsServiceError;

const RUNTIME_EXCEPTION: &str = "java.lang.RuntimeException";
const IO_EXCEPTION: &str = "java.io.IOException";
const ATHENA_CONNECTOR_EXCEPTION: &str =
    "com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException";

#[derive(Clone, Copy)]
pub enum JavaCause<'a> {
    Federation(&'a FederationError),
    Error(&'a (dyn StdError + 'static)),
}

impl<'a> JavaCause<'a> {
    fn of(error: &'a (dyn StdError + 'static)) -> JavaCause<'a> {
        let federation = error.downcast_ref::<FederationError>().or_else(|| {
            error
                .downcast_ref::<Arc<FederationError>>()
                .map(AsRef::as_ref)
        });
        match federation {
            Some(federation) => JavaCause::Federation(federation),
            None => JavaCause::Error(error),
        }
    }

    pub fn java_class_name(self) -> String {
        match self {
            JavaCause::Federation(federation) => federation.java_class_name(),
            JavaCause::Error(error) => {
                if error.is::<AthenaConnectorException>() {
                    ATHENA_CONNECTOR_EXCEPTION.to_string()
                } else if let Some(aws) = error.downcast_ref::<AwsServiceError>() {
                    FederationError::AwsService(aws.clone()).java_class_name()
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
            JavaCause::Federation(federation) => federation.java_message(),
            JavaCause::Error(error) => Some(error.to_string()),
        }
    }

    pub fn java_cause(self) -> Option<JavaCause<'a>> {
        match self {
            JavaCause::Federation(federation) => federation.java_cause(),
            JavaCause::Error(error) => error.source().map(JavaCause::of),
        }
    }
}

impl FederationError {
    pub fn java_cause(&self) -> Option<JavaCause<'_>> {
        let source = match self {
            FederationError::RuntimeWrapper { source } => {
                return Some(JavaCause::Federation(source));
            }
            FederationError::Wrapped { source, .. } => source.source(),
            FederationError::IoError(source) => source.source(),
            FederationError::Arrow(source) => source.source(),
            FederationError::AthenaConnector(e) => e.source(),
            FederationError::Throttle(e) => e.source(),
            _ => None,
        };
        source.map(JavaCause::of)
    }
}
