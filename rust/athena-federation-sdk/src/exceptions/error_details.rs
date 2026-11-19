use std::fmt;

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
    pub fn from_http_status(status: u16) -> FederationSourceErrorCode {
        match status {
            400 | 409 => FederationSourceErrorCode::InvalidInputException,
            401 => FederationSourceErrorCode::InvalidCredentialsException,
            403 => FederationSourceErrorCode::AccessDeniedException,
            404 => FederationSourceErrorCode::EntityNotFoundException,
            408 | 504 => FederationSourceErrorCode::OperationTimeoutException,
            429 => FederationSourceErrorCode::ThrottlingException,
            500 | 502 | 503 => FederationSourceErrorCode::InternalServiceException,
            501 => FederationSourceErrorCode::OperationNotSupportedException,
            other => {
                tracing::warn!("Unknown error code: {}", other);
                FederationSourceErrorCode::InvalidInputException
            }
        }
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
}

impl fmt::Display for FederationSourceErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
