use std::fmt;

use athena_federation_sdk::Result;

use crate::java_null::OrJavaNull;
use crate::validate::not_blank;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DatabaseConnectionConfig {
    catalog: String,
    engine: String,
    jdbc_connection_string: String,
    secret: Option<String>,
}

impl DatabaseConnectionConfig {
    pub fn new(
        catalog: &str,
        engine: &str,
        jdbc_connection_string: &str,
    ) -> Result<DatabaseConnectionConfig> {
        Ok(DatabaseConnectionConfig {
            catalog: not_blank(Some(catalog), "catalog must not be blank")?.to_string(),
            engine: not_blank(Some(engine), "engine must not be blank")?.to_string(),
            jdbc_connection_string: not_blank(
                Some(jdbc_connection_string),
                "jdbcConnectionString must not be blank",
            )?
            .to_string(),
            secret: None,
        })
    }

    pub fn new_with_secret(
        catalog: &str,
        engine: &str,
        jdbc_connection_string: &str,
        secret: &str,
    ) -> Result<DatabaseConnectionConfig> {
        let mut config = DatabaseConnectionConfig::new(catalog, engine, jdbc_connection_string)?;
        config.secret = Some(not_blank(Some(secret), "secret must not be blank")?.to_string());
        Ok(config)
    }

    pub fn engine(&self) -> &str {
        &self.engine
    }

    pub fn jdbc_connection_string(&self) -> &str {
        &self.jdbc_connection_string
    }

    pub fn catalog(&self) -> &str {
        &self.catalog
    }

    pub fn secret(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

impl fmt::Display for DatabaseConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DatabaseConnectionConfig{{catalog='{}', engine={}, jdbcConnectionString='{}', secret='{}'}}",
            self.catalog,
            self.engine,
            self.jdbc_connection_string,
            self.secret.as_deref().or_java_null()
        )
    }
}
