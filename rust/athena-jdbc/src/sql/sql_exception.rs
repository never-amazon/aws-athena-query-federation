use std::error::Error;
use std::fmt;
use std::sync::Arc;

use athena_federation_sdk::exceptions::JavaExceptionKind;
use athena_federation_sdk::FederationError;

pub const SQL_EXCEPTION_CLASS: &str = "java.sql.SQLException";

#[derive(Debug, Clone)]
pub struct SqlException {
    class_name: &'static str,
    message: String,
    sql_state: Option<String>,
    error_code: i32,
    cause: Option<Arc<dyn Error + Send + Sync>>,
}

impl SqlException {
    pub fn new(message: impl Into<String>) -> SqlException {
        SqlException::with_class_name(SQL_EXCEPTION_CLASS, message)
    }

    pub fn with_class_name(class_name: &'static str, message: impl Into<String>) -> SqlException {
        SqlException {
            class_name,
            message: message.into(),
            sql_state: None,
            error_code: 0,
            cause: None,
        }
    }

    #[must_use]
    pub fn with_sql_state(mut self, sql_state: impl Into<String>) -> SqlException {
        self.sql_state = Some(sql_state.into());
        self
    }

    #[must_use]
    pub fn with_error_code(mut self, error_code: i32) -> SqlException {
        self.error_code = error_code;
        self
    }

    #[must_use]
    pub fn with_cause(mut self, cause: impl Error + Send + Sync + 'static) -> SqlException {
        self.cause = Some(Arc::new(cause));
        self
    }

    pub fn class_name(&self) -> &'static str {
        self.class_name
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn sql_state(&self) -> Option<&str> {
        self.sql_state.as_deref()
    }

    pub fn error_code(&self) -> i32 {
        self.error_code
    }

    pub fn from_federation_error(error: &FederationError) -> Option<&SqlException> {
        match error {
            FederationError::Wrapped { source, .. } => source.downcast_ref::<SqlException>(),
            _ => None,
        }
    }
}

impl PartialEq for SqlException {
    fn eq(&self, other: &Self) -> bool {
        self.class_name == other.class_name
            && self.message == other.message
            && self.sql_state == other.sql_state
            && self.error_code == other.error_code
    }
}

impl fmt::Display for SqlException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for SqlException {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|c| c as &(dyn Error + 'static))
    }
}

impl From<SqlException> for FederationError {
    fn from(error: SqlException) -> Self {
        FederationError::wrapped(JavaExceptionKind::Checked(error.class_name), error)
    }
}
