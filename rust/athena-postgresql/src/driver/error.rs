use std::io;

use athena_jdbc::sql::SqlError;

pub(crate) const CONNECTION_UNABLE_TO_CONNECT: &str = "08001";
pub(crate) const CONNECTION_DOES_NOT_EXIST: &str = "08003";
pub(crate) const CONNECTION_REJECTED: &str = "08004";
pub(crate) const CONNECTION_FAILURE: &str = "08006";
pub(crate) const PROTOCOL_VIOLATION: &str = "08P01";
pub(crate) const INVALID_PARAMETER_VALUE: &str = "22023";
pub(crate) const INVALID_TRANSACTION_STATE: &str = "25000";
pub(crate) const INVALID_AUTHORIZATION: &str = "28000";
pub(crate) const NO_DATA: &str = "02000";
pub(crate) const DATA_TYPE_MISMATCH: &str = "42821";

pub(crate) fn psql(message: impl Into<String>, sql_state: &str) -> SqlError {
    SqlError::postgres(message, sql_state)
}

pub(crate) fn psql_with_cause(
    message: impl Into<String>,
    sql_state: &str,
    cause: io::Error,
) -> SqlError {
    psql(message, sql_state).with_cause(cause)
}

pub(crate) fn io_failure(cause: io::Error) -> SqlError {
    psql_with_cause(
        "An I/O error occurred while sending to the backend.",
        CONNECTION_FAILURE,
        cause,
    )
}

pub(crate) fn connection_closed() -> SqlError {
    psql(
        "This connection has been closed.",
        CONNECTION_DOES_NOT_EXIST,
    )
}

pub(crate) fn protocol_violation(message: impl Into<String>) -> SqlError {
    psql(message, PROTOCOL_VIOLATION)
}

fn field(fields: &[(u8, String)], code: u8) -> Option<&str> {
    fields
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

pub(crate) fn server_error_message(fields: &[(u8, String)]) -> String {
    let mut message = String::new();
    if let Some(severity) = field(fields, b'S') {
        message.push_str(severity);
        message.push_str(": ");
    }
    if let Some(text) = field(fields, b'M') {
        message.push_str(text);
    }
    for (code, label) in [
        (b'D', "Detail"),
        (b'H', "Hint"),
        (b'P', "Position"),
        (b'W', "Where"),
    ] {
        if let Some(value) = field(fields, code) {
            message.push_str("\n  ");
            message.push_str(label);
            message.push_str(": ");
            message.push_str(value);
        }
    }
    message
}

pub(crate) fn server_error(fields: &[(u8, String)]) -> SqlError {
    let state = field(fields, b'C').unwrap_or_default().to_string();
    SqlError::postgres(server_error_message(fields), state)
}
