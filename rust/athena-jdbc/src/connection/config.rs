use std::collections::HashMap;
use std::fmt;
use std::hash::BuildHasher;
use std::sync::LazyLock;

use athena_federation_sdk::connection::environment_constants;
use athena_federation_sdk::{FederationError, Result};
use indexmap::IndexMap;
use regex::Regex;

use crate::java_null::OrJavaNull;
use crate::validate::{is_blank, is_true, not_blank};

pub const CONNECTION_STRING_PROPERTY_SUFFIX: &str = "_connection_string";
pub const DEFAULT_CONNECTION_STRING_PROPERTY: &str = "default";
const MUX_CATALOG_LIMIT: usize = 100;

static CONNECTION_STRING_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("([a-zA-Z0-9]+)://(.*)").expect("the pattern is a valid regex"));

static SECRET_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{(([a-z-]+!)?[a-zA-Z0-9:/_+=.@-]+)\}").expect("the pattern is a valid regex")
});

#[derive(Debug, Clone, Default)]
pub struct DatabaseConnectionConfigBuilder {
    properties: Option<IndexMap<String, String>>,
    engine: Option<String>,
}

impl DatabaseConnectionConfigBuilder {
    pub fn new() -> DatabaseConnectionConfigBuilder {
        DatabaseConnectionConfigBuilder::default()
    }

    pub fn secret_pattern() -> &'static Regex {
        &SECRET_PATTERN
    }

    pub fn build_from_system_env<I, K, V>(
        database_engine: &str,
        config_options: I,
    ) -> Result<Vec<DatabaseConnectionConfig>>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        DatabaseConnectionConfigBuilder::new()
            .properties(config_options)
            .engine(database_engine)
            .build()
    }

    #[must_use]
    pub fn engine(mut self, engine: impl Into<String>) -> DatabaseConnectionConfigBuilder {
        self.engine = Some(engine.into());
        self
    }

    #[must_use]
    pub fn properties<I, K, V>(mut self, properties: I) -> DatabaseConnectionConfigBuilder
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.properties = Some(
            properties
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        );
        self
    }

    pub fn build(&self) -> Result<Vec<DatabaseConnectionConfig>> {
        let properties = self
            .properties
            .as_ref()
            .ok_or_else(|| FederationError::null_pointer("properties must not be empty"))?;
        is_true(!properties.is_empty(), "properties must not be empty")?;
        is_true(
            properties.contains_key(DEFAULT_CONNECTION_STRING_PROPERTY),
            "Default connection string must be present",
        )?;
        let mut database_connection_configs = Vec::new();
        let mut number_of_catalogs = 0usize;
        let glue_connection_blank = is_blank(
            properties
                .get(environment_constants::DEFAULT_GLUE_CONNECTION)
                .map(String::as_str),
        );
        for (key, value) in properties {
            let catalog_name = if key.to_lowercase() == DEFAULT_CONNECTION_STRING_PROPERTY {
                key.to_lowercase()
            } else if key.ends_with(CONNECTION_STRING_PROPERTY_SUFFIX) {
                key.replace(CONNECTION_STRING_PROPERTY_SUFFIX, "")
            } else {
                continue;
            };
            database_connection_configs
                .push(self.extract_database_connection_config(&catalog_name, value)?);
            if glue_connection_blank {
                number_of_catalogs += 1;
            }
            if number_of_catalogs > MUX_CATALOG_LIMIT {
                return Err(FederationError::invalid_input(format!(
                    "Too many database instances in mux. Max supported is {MUX_CATALOG_LIMIT}"
                )));
            }
        }
        Ok(database_connection_configs)
    }

    fn extract_database_connection_config(
        &self,
        catalog_name: &str,
        connection_string: &str,
    ) -> Result<DatabaseConnectionConfig> {
        let captures = CONNECTION_STRING_PATTERN
            .captures(connection_string)
            .ok_or_else(|| {
                FederationError::invalid_input(format!(
                    "Invalid connection String for Catalog {catalog_name}"
                ))
            })?;
        let db_type = captures.get(1).map_or("", |m| m.as_str());
        let jdbc_connection_string = captures.get(2).map_or("", |m| m.as_str());
        not_blank(Some(db_type), "Database type must not be blank.")?;
        not_blank(
            Some(jdbc_connection_string),
            "JDBC Connection string must not be blank.",
        )?;
        is_true(
            self.engine.as_deref() == Some(db_type),
            "JDBC Connection string must be prepended by correct database type.",
        )?;
        let engine = self.engine.as_deref().unwrap_or_default();
        match extract_secret_name(jdbc_connection_string) {
            Some(secret) => DatabaseConnectionConfig::new_with_secret(
                catalog_name,
                engine,
                jdbc_connection_string,
                secret,
            ),
            None => DatabaseConnectionConfig::new(catalog_name, engine, jdbc_connection_string),
        }
    }
}

fn extract_secret_name(jdbc_connection_string: &str) -> Option<&str> {
    SECRET_PATTERN
        .captures(jdbc_connection_string)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .filter(|s| !s.trim().is_empty())
}

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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DatabaseConnectionInfo {
    driver_class_name: String,
    default_port: i32,
}

impl DatabaseConnectionInfo {
    pub fn new(driver_class_name: &str, default_port: i32) -> Result<DatabaseConnectionInfo> {
        Ok(DatabaseConnectionInfo {
            driver_class_name: not_blank(
                Some(driver_class_name),
                "driverClassName must not be blank",
            )?
            .to_string(),
            default_port,
        })
    }

    pub fn driver_class_name(&self) -> &str {
        &self.driver_class_name
    }

    pub fn default_port(&self) -> i32 {
        self.default_port
    }
}

pub(crate) fn owned<S: BuildHasher>(
    config_options: &HashMap<String, String, S>,
) -> HashMap<String, String> {
    config_options
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn single_database_config_from_env<S: BuildHasher>(
    database_engine: &str,
    config_options: &HashMap<String, String, S>,
) -> Result<DatabaseConnectionConfig> {
    let configs = DatabaseConnectionConfigBuilder::build_from_system_env(
        database_engine,
        owned(config_options),
    )?;
    configs
        .into_iter()
        .find(|config| config.catalog() == DEFAULT_CONNECTION_STRING_PROPERTY && config.engine() == database_engine)
        .ok_or_else(|| {
            FederationError::invalid_input(format!(
                "Must provide default connection string parameter {DEFAULT_CONNECTION_STRING_PROPERTY} for database type {database_engine}"
            ))
        })
}
