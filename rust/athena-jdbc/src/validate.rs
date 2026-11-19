use athena_federation_sdk::{FederationError, Result};

use crate::sql::JdbcConnection;

pub(crate) fn not_blank<'a>(value: Option<&'a str>, message: &str) -> Result<&'a str> {
    match value {
        None => Err(FederationError::null_pointer(message)),
        Some(v) if v.trim().is_empty() => {
            Err(FederationError::IllegalArgument(message.to_string()))
        }
        Some(v) => Ok(v),
    }
}

pub(crate) fn is_true(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(FederationError::IllegalArgument(message.to_string()))
    }
}

pub(crate) fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|v| v.trim().is_empty())
}

pub(crate) fn runtime_with_cause(
    message: impl Into<String>,
    cause: FederationError,
) -> FederationError {
    FederationError::runtime_with_cause(message, cause)
}

pub(crate) fn null_pointer(message: impl Into<String>) -> FederationError {
    FederationError::null_pointer(message)
}

pub async fn close_after<T>(connection: &dyn JdbcConnection, result: Result<T>) -> Result<T> {
    let closed = connection.close().await;
    match (result, closed) {
        (Err(e), _) => Err(e),
        (Ok(_), Err(e)) => Err(e.into()),
        (Ok(v), Ok(())) => Ok(v),
    }
}
