use std::collections::HashMap;

use athena_federation_sdk::connection::{environment_constants, EnvironmentProperties};
use athena_jdbc::JdbcEnvironmentProperties;

use crate::snowflake_constants::java_trim;

const WAREHOUSE_PROPERTY_KEY: &str = "warehouse";
const DB_PROPERTY_KEY: &str = "db";
const SCHEMA_PROPERTY_KEY: &str = "schema";
const SNOWFLAKE_ESCAPE_CHARACTER: &str = "\"";
const NULL_LITERAL: &str = "null";
const SNOWFLAKE_JDBC_PARAMETER_PREFIX: &str = "/?";

#[derive(Clone, Default)]
pub struct SnowflakeEnvironmentProperties {
    environment_properties: EnvironmentProperties,
}

impl SnowflakeEnvironmentProperties {
    pub fn new() -> SnowflakeEnvironmentProperties {
        SnowflakeEnvironmentProperties::default()
    }

    pub fn with_environment_properties(
        environment_properties: EnvironmentProperties,
    ) -> SnowflakeEnvironmentProperties {
        SnowflakeEnvironmentProperties {
            environment_properties,
        }
    }

    pub fn get_snow_flake_parameter(
        base_property: &HashMap<String, String>,
        connection_properties: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        tracing::debug!("getSnowFlakeParameter, Loading connection properties");
        let mut parameters = base_property.clone();
        if !connection_properties.contains_key(environment_constants::DEFAULT_GLUE_CONNECTION) {
            return parameters;
        }
        if !connection_properties.contains_key(environment_constants::SCHEMA) {
            tracing::debug!("No schema specified in connection string");
        }
        copy_usable(
            connection_properties
                .get(environment_constants::WAREHOUSE)
                .map(String::as_str),
            WAREHOUSE_PROPERTY_KEY,
            "warehouse",
            &mut parameters,
        );
        copy_usable(
            connection_properties
                .get(environment_constants::DATABASE)
                .map(String::as_str),
            DB_PROPERTY_KEY,
            "database",
            &mut parameters,
        );
        copy_usable(
            connection_properties
                .get(environment_constants::SCHEMA)
                .map(String::as_str),
            SCHEMA_PROPERTY_KEY,
            "schema",
            &mut parameters,
        );
        parameters
    }
}

fn copy_usable(
    value: Option<&str>,
    key: &str,
    label: &str,
    parameters: &mut HashMap<String, String>,
) {
    match value {
        Some(value) if is_usable_value(value) => {
            parameters.insert(
                key.to_string(),
                format!("{SNOWFLAKE_ESCAPE_CHARACTER}{value}{SNOWFLAKE_ESCAPE_CHARACTER}"),
            );
        }
        Some(_) => tracing::warn!(
            "Ignoring unusable {label} value; the session will fall back to the user's default {label}"
        ),
        None => {}
    }
}

fn is_usable_value(value: &str) -> bool {
    let trimmed = java_trim(value);
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(NULL_LITERAL)
}

fn is_valid_port(port_value: &str) -> bool {
    !port_value.is_empty()
        && java_trim(port_value)
            .parse::<i32>()
            .is_ok_and(|port| port > 0)
}

impl JdbcEnvironmentProperties for SnowflakeEnvironmentProperties {
    fn environment_properties(&self) -> &EnvironmentProperties {
        &self.environment_properties
    }

    fn get_connection_string_prefix(
        &self,
        _connection_properties: &HashMap<String, String>,
    ) -> String {
        "snowflake://jdbc:snowflake://".to_string()
    }

    fn connection_properties_to_environment(
        &self,
        connection_properties: HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut environment = HashMap::new();
        for key in [
            environment_constants::WAREHOUSE,
            environment_constants::DATABASE,
            environment_constants::SCHEMA,
        ] {
            if let Some(value) = connection_properties.get(key) {
                environment.insert(key.to_string(), value.clone());
            }
        }
        let mut connection_string = self.get_connection_string_prefix(&connection_properties);
        connection_string.push_str(
            connection_properties
                .get(environment_constants::HOST)
                .map_or("null", String::as_str),
        );
        if let Some(raw_port) = connection_properties.get(environment_constants::PORT) {
            let port_value = java_trim(raw_port);
            if is_valid_port(port_value) {
                connection_string.push(':');
                connection_string.push_str(port_value);
            } else {
                tracing::warn!(
                    "Invalid port value '{raw_port}' provided, skipping port in connection string"
                );
            }
        }
        let jdbc_parameters = self.get_jdbc_parameters(&connection_properties);
        if !jdbc_parameters.is_empty() && jdbc_parameters != self.get_jdbc_parameters_separator() {
            tracing::info!("JDBC parameters found, adding to JDBC String");
            connection_string.push_str(SNOWFLAKE_JDBC_PARAMETER_PREFIX);
            connection_string.push_str(&jdbc_parameters);
        }
        environment.insert(
            environment_constants::DEFAULT.to_string(),
            connection_string,
        );
        environment
    }

    fn get_database(&self, _connection_properties: &HashMap<String, String>) -> String {
        String::new()
    }

    fn get_jdbc_parameters_separator(&self) -> String {
        "&".to_string()
    }
}
