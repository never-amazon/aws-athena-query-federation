use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct FederationThrottleException {
    message: Option<String>,
    cause: Option<Arc<dyn StdError + Send + Sync>>,
}

impl FederationThrottleException {
    pub fn new() -> Self {
        FederationThrottleException::default()
    }

    pub fn with_message(message: impl Into<String>) -> Self {
        FederationThrottleException {
            message: Some(message.into()),
            cause: None,
        }
    }

    pub fn with_message_and_cause(message: impl Into<String>, cause: Arc<dyn StdError + Send + Sync>) -> Self {
        FederationThrottleException {
            message: Some(message.into()),
            cause: Some(cause),
        }
    }

    pub fn get_message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub fn get_cause(&self) -> Option<&Arc<dyn StdError + Send + Sync>> {
        self.cause.as_ref()
    }

    pub fn get_suppressed(&self) -> &[Arc<dyn StdError + Send + Sync>] {
        &[]
    }
}

impl fmt::Debug for FederationThrottleException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FederationThrottleException")
            .field("message", &self.message)
            .field("cause", &self.cause.as_ref().map(|c| c.to_string()))
            .finish()
    }
}

impl fmt::Display for FederationThrottleException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message.as_deref().unwrap_or("FederationThrottleException"))
    }
}

impl StdError for FederationThrottleException {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause.as_ref().map(|c| c.as_ref() as &(dyn StdError + 'static))
    }
}
