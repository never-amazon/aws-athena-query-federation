use std::error::Error;
use std::sync::Arc;

use athena_federation_sdk::exceptions::{JavaCause, JavaThrowable};
use athena_federation_sdk::FederationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Driver {
    #[default]
    Jdbc,
    Postgres,
    Snowflake,
}

impl Driver {
    pub fn error(self, message: impl Into<String>) -> SqlError {
        let message = message.into();
        match self {
            Driver::Jdbc => SqlError::new(message),
            Driver::Postgres => SqlError::Postgres {
                message,
                sql_state: None,
                cause: None,
            },
            Driver::Snowflake => SqlError::snowflake(message),
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SqlError {
    #[error("{message}")]
    Jdbc {
        message: String,
        sql_state: Option<String>,
        code: Option<i32>,
    },
    #[error("{message}")]
    Postgres {
        message: String,
        sql_state: Option<String>,
        #[source]
        cause: Option<Arc<dyn Error + Send + Sync>>,
    },
    #[error("{message}")]
    Snowflake {
        message: String,
        sql_state: Option<String>,
        code: Option<i32>,
    },
}

impl SqlError {
    pub fn new(message: impl Into<String>) -> SqlError {
        SqlError::Jdbc {
            message: message.into(),
            sql_state: None,
            code: None,
        }
    }

    pub fn postgres(message: impl Into<String>, sql_state: impl Into<String>) -> SqlError {
        SqlError::Postgres {
            message: message.into(),
            sql_state: Some(sql_state.into()),
            cause: None,
        }
    }

    pub fn snowflake(message: impl Into<String>) -> SqlError {
        SqlError::Snowflake {
            message: message.into(),
            sql_state: None,
            code: None,
        }
    }

    #[must_use]
    pub fn with_sql_state(mut self, state: impl Into<String>) -> SqlError {
        match &mut self {
            SqlError::Jdbc { sql_state, .. }
            | SqlError::Postgres { sql_state, .. }
            | SqlError::Snowflake { sql_state, .. } => *sql_state = Some(state.into()),
        }
        self
    }

    #[must_use]
    pub fn with_cause(self, cause: impl Error + Send + Sync + 'static) -> SqlError {
        match self {
            SqlError::Postgres {
                message, sql_state, ..
            } => SqlError::Postgres {
                message,
                sql_state,
                cause: Some(Arc::new(cause)),
            },
            other => other,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            SqlError::Jdbc { message, .. }
            | SqlError::Postgres { message, .. }
            | SqlError::Snowflake { message, .. } => message,
        }
    }

    pub fn sql_state(&self) -> Option<&str> {
        match self {
            SqlError::Jdbc { sql_state, .. }
            | SqlError::Postgres { sql_state, .. }
            | SqlError::Snowflake { sql_state, .. } => sql_state.as_deref(),
        }
    }

    pub fn error_code(&self) -> i32 {
        match self {
            SqlError::Jdbc { code, .. } | SqlError::Snowflake { code, .. } => code.unwrap_or(0),
            SqlError::Postgres { .. } => 0,
        }
    }

    pub fn from_federation_error(error: &FederationError) -> Option<&SqlError> {
        error.downcast_external::<SqlError>()
    }
}

impl PartialEq for SqlError {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
            && self.message() == other.message()
            && self.sql_state() == other.sql_state()
            && self.error_code() == other.error_code()
    }
}

impl JavaThrowable for SqlError {
    fn java_class_name(&self) -> String {
        let name = match self {
            SqlError::Jdbc { .. } => "java.sql.SQLException",
            SqlError::Postgres { .. } => "org.postgresql.util.PSQLException",
            SqlError::Snowflake { .. } => {
                "net.snowflake.client.api.exception.SnowflakeSQLException"
            }
        };
        name.to_string()
    }

    fn java_cause(&self) -> Option<JavaCause<'_>> {
        match self {
            SqlError::Postgres {
                cause: Some(cause), ..
            } => Some(JavaCause::foreign(cause.as_ref())),
            _ => None,
        }
    }

    fn is_runtime_exception(&self) -> bool {
        false
    }
}

impl From<SqlError> for FederationError {
    fn from(error: SqlError) -> Self {
        FederationError::external(error)
    }
}
