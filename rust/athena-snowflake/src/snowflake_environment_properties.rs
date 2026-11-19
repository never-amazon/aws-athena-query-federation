use std::collections::HashMap;
use std::hash::BuildHasher;

use athena_federation_sdk::connection::environment_constants;
use athena_jdbc::ConnectionStringFormat;

use crate::snowflake_constants::java_trim;
use athena_jdbc::java_null::{OrJavaNull, JAVA_NULL};

const WAREHOUSE_PROPERTY_KEY: &str = "warehouse";
const DB_PROPERTY_KEY: &str = "db";
const SCHEMA_PROPERTY_KEY: &str = "schema";
const SNOWFLAKE_ESCAPE_CHARACTER: &str = "\"";
const SNOWFLAKE_JDBC_PARAMETER_PREFIX: &str = "/?";

pub const SNOWFLAKE_CONNECTION_STRING: ConnectionStringFormat = ConnectionStringFormat {
    prefix: "snowflake://jdbc:snowflake://",
    database_separator: None,
    params_separator: "&",
    delimiter: "&",
};

pub fn snowflake_parameters<B: BuildHasher, S: BuildHasher>(
    base_property: &HashMap<String, String, B>,
    connection_properties: &HashMap<String, String, S>,
) -> HashMap<String, String> {
    tracing::debug!("getSnowFlakeParameter, Loading connection properties");
    let mut parameters: HashMap<String, String> = base_property
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
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
    !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case(JAVA_NULL)
}

fn is_valid_port(port_value: &str) -> bool {
    !port_value.is_empty()
        && java_trim(port_value)
            .parse::<i32>()
            .is_ok_and(|port| port > 0)
}

pub fn connection_properties_to_environment<S: BuildHasher>(
    connection_properties: &HashMap<String, String, S>,
) -> HashMap<String, String> {
    let format = SNOWFLAKE_CONNECTION_STRING;
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
    let mut connection_string = format.prefix.to_string();
    connection_string.push_str(
        connection_properties
            .get(environment_constants::HOST)
            .or_java_null(),
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
    let jdbc_parameters = format.jdbc_parameters(connection_properties);
    if !jdbc_parameters.is_empty() && jdbc_parameters != format.params_separator {
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
