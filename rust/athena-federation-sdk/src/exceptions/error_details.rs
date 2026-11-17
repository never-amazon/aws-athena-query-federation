use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FederationSourceErrorCode {
    AccessDeniedException,
    EntityNotFoundException,
    InvalidCredentialsException,
    InvalidInputException,
    InvalidResponseException,
    OperationTimeoutException,
    OperationNotSupportedException,
    InternalServiceException,
    PartialFailureException,
    ThrottlingException,
    UnknownToSdkVersion,
}

impl FederationSourceErrorCode {
    pub fn values() -> &'static [FederationSourceErrorCode] {
        use FederationSourceErrorCode::{
            AccessDeniedException, EntityNotFoundException, InternalServiceException,
            InvalidCredentialsException, InvalidInputException, InvalidResponseException,
            OperationNotSupportedException, OperationTimeoutException, PartialFailureException,
            ThrottlingException,
        };
        &[
            AccessDeniedException,
            EntityNotFoundException,
            InvalidCredentialsException,
            InvalidInputException,
            InvalidResponseException,
            OperationTimeoutException,
            OperationNotSupportedException,
            InternalServiceException,
            PartialFailureException,
            ThrottlingException,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        use FederationSourceErrorCode::{
            AccessDeniedException, EntityNotFoundException, InternalServiceException,
            InvalidCredentialsException, InvalidInputException, InvalidResponseException,
            OperationNotSupportedException, OperationTimeoutException, PartialFailureException,
            ThrottlingException, UnknownToSdkVersion,
        };
        match self {
            AccessDeniedException => "AccessDeniedException",
            EntityNotFoundException => "EntityNotFoundException",
            InvalidCredentialsException => "InvalidCredentialsException",
            InvalidInputException => "InvalidInputException",
            InvalidResponseException => "InvalidResponseException",
            OperationTimeoutException => "OperationTimeoutException",
            OperationNotSupportedException => "OperationNotSupportedException",
            InternalServiceException => "InternalServiceException",
            PartialFailureException => "PartialFailureException",
            ThrottlingException => "ThrottlingException",
            UnknownToSdkVersion => "UNKNOWN_TO_SDK_VERSION",
        }
    }

    pub fn from_value(value: &str) -> FederationSourceErrorCode {
        FederationSourceErrorCode::values()
            .iter()
            .copied()
            .find(|code| code.as_str() == value)
            .unwrap_or(FederationSourceErrorCode::UnknownToSdkVersion)
    }
}

impl fmt::Display for FederationSourceErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FederationSourceErrorCode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(FederationSourceErrorCode::from_value(s))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ErrorDetails {
    error_code: Option<String>,
    error_message: Option<String>,
}

impl ErrorDetails {
    pub fn builder() -> ErrorDetailsBuilder {
        ErrorDetailsBuilder::default()
    }

    pub fn of(code: FederationSourceErrorCode) -> ErrorDetails {
        ErrorDetails::builder().error_code(code.as_str()).build()
    }

    pub fn of_with_message(
        code: FederationSourceErrorCode,
        message: impl Into<String>,
    ) -> ErrorDetails {
        ErrorDetails::builder()
            .error_code(code.as_str())
            .error_message(message)
            .build()
    }

    pub fn error_code(&self) -> Option<&str> {
        self.error_code.as_deref()
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn to_builder(&self) -> ErrorDetailsBuilder {
        ErrorDetailsBuilder {
            error_code: self.error_code.clone(),
            error_message: self.error_message.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ErrorDetailsBuilder {
    error_code: Option<String>,
    error_message: Option<String>,
}

impl ErrorDetailsBuilder {
    #[must_use]
    pub fn error_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }

    #[must_use]
    pub fn error_message(mut self, message: impl Into<String>) -> Self {
        self.error_message = Some(message.into());
        self
    }

    pub fn build(self) -> ErrorDetails {
        ErrorDetails {
            error_code: self.error_code,
            error_message: self.error_message,
        }
    }
}
