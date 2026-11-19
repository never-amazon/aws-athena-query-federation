use std::fmt;
use std::sync::Arc;

use super::java_throwable::JavaThrowable;

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
