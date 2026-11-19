pub(crate) mod federation_error;
mod java_throwable;

pub use federation_error::{
    AthenaConnectorException, FederationSourceErrorCode, FederationThrottleException,
};
pub(crate) use federation_error::{FederationError, Result};
pub use java_throwable::{class_cast_message, JavaCause, JavaThrowable};
