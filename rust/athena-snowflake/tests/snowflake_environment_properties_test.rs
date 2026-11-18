use std::collections::HashMap;

use athena_federation_sdk::connection::environment_constants;
use athena_jdbc::JdbcEnvironmentProperties;
use athena_snowflake::{SnowflakeConstants, SnowflakeEnvironmentProperties};

const DEFAULT_WAREHOUSE: &str = "warehouse";
const TEST_WAREHOUSE: &str = "test_warehouse";
const TEST_DATABASE: &str = "test_database";
const TEST_SCHEMA: &str = "test_schema";
const TEST_HOST: &str = "test_host";
const TEST_PORT: &str = "443";
const TEST_USER: &str = "test_user";
const SNOWFLAKE_JDBC_SNOWFLAKE: &str = "snowflake://jdbc:snowflake://";
const DB: &str = "db";
const DEFAULT_SCHEMA: &str = "schema";

const WAREHOUSE: &str = environment_constants::WAREHOUSE;
const DATABASE: &str = environment_constants::DATABASE;
const SCHEMA: &str = environment_constants::SCHEMA;
const HOST: &str = environment_constants::HOST;
const PORT: &str = environment_constants::PORT;
const DEFAULT: &str = environment_constants::DEFAULT;
const GLUE: &str = environment_constants::DEFAULT_GLUE_CONNECTION;
const USER: &str = SnowflakeConstants::USER;

fn map(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn get<'a>(map: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    map.get(key).map(String::as_str)
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

fn to_environment(properties: &HashMap<String, String>) -> HashMap<String, String> {
    SnowflakeEnvironmentProperties::new().connection_properties_to_environment(properties.clone())
}

fn base_property() -> HashMap<String, String> {
    map(&[(USER, TEST_USER)])
}

#[test]
fn connection_properties_to_environment_with_all_properties_returns_correct_environment() {
    let properties = map(&[
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
        (HOST, TEST_HOST),
        (PORT, TEST_PORT),
    ]);

    let environment = to_environment(&properties);

    assert_eq!(Some(TEST_WAREHOUSE), get(&environment, WAREHOUSE));
    assert_eq!(Some(TEST_DATABASE), get(&environment, DATABASE));
    assert_eq!(Some(TEST_SCHEMA), get(&environment, SCHEMA));
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{TEST_HOST}:{TEST_PORT}");
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn connection_properties_to_environment_without_port_returns_environment_without_port() {
    let properties = map(&[
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
        (HOST, TEST_HOST),
    ]);

    let environment = to_environment(&properties);

    assert_eq!(Some(TEST_WAREHOUSE), get(&environment, WAREHOUSE));
    assert_eq!(Some(TEST_DATABASE), get(&environment, DATABASE));
    assert_eq!(Some(TEST_SCHEMA), get(&environment, SCHEMA));
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{TEST_HOST}");
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn connection_properties_to_environment_with_invalid_port_values_skips_invalid_ports() {
    let env_props = SnowflakeEnvironmentProperties::new();
    let mut properties = map(&[
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
        (HOST, TEST_HOST),
    ]);
    let without_port = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{TEST_HOST}");
    let cases = [
        ("abc", without_port.clone()),
        ("-1", without_port.clone()),
        ("0", without_port.clone()),
        ("", without_port.clone()),
        ("8080@test", without_port.clone()),
        (" 8080 ", format!("{without_port}:8080")),
        ("443", format!("{without_port}:443")),
        ("65535", format!("{without_port}:65535")),
    ];
    for (port, expected) in cases {
        properties.insert(PORT.to_string(), port.to_string());
        let environment = env_props.connection_properties_to_environment(properties.clone());
        assert_eq!(
            Some(expected.as_str()),
            get(&environment, DEFAULT),
            "port {port:?}"
        );
    }
}

#[test]
fn connection_properties_to_environment_with_null_values_excludes_null_properties() {
    let properties = map(&[(HOST, TEST_HOST)]);

    let environment = to_environment(&properties);

    assert!(!environment.contains_key(WAREHOUSE));
    assert!(!environment.contains_key(DATABASE));
    assert!(!environment.contains_key(SCHEMA));
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{TEST_HOST}");
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn get_connection_string_prefix_with_properties_returns_snowflake_jdbc_prefix() {
    let prefix =
        SnowflakeEnvironmentProperties::new().get_connection_string_prefix(&HashMap::new());
    assert_eq!(SNOWFLAKE_JDBC_SNOWFLAKE, prefix);
}

#[test]
fn get_database_with_properties_returns_empty_string() {
    let database = SnowflakeEnvironmentProperties::new().get_database(&HashMap::new());
    assert_eq!("", database);
}

#[test]
fn get_jdbc_parameters_separator_always_returns_ampersand() {
    let separator = SnowflakeEnvironmentProperties::new().get_jdbc_parameters_separator();
    assert_eq!("&", separator);
}

#[test]
fn get_snow_flake_parameter_with_glue_connection_returns_all_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert_eq!(
        Some(quoted(TEST_WAREHOUSE).as_str()),
        get(&parameters, DEFAULT_WAREHOUSE)
    );
    assert_eq!(Some(quoted(TEST_DATABASE).as_str()), get(&parameters, DB));
    assert_eq!(
        Some(quoted(TEST_SCHEMA).as_str()),
        get(&parameters, DEFAULT_SCHEMA)
    );
}

#[test]
fn get_snow_flake_parameter_without_glue_connection_excludes_warehouse_database_schema() {
    let connection_properties = map(&[
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert!(!parameters.contains_key(DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_without_schema_excludes_schema_parameter() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, TEST_WAREHOUSE),
        (DATABASE, TEST_DATABASE),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert_eq!(
        Some(quoted(TEST_WAREHOUSE).as_str()),
        get(&parameters, DEFAULT_WAREHOUSE)
    );
    assert_eq!(Some(quoted(TEST_DATABASE).as_str()), get(&parameters, DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_null_values_excludes_null_parameters() {
    let connection_properties = map(&[(GLUE, "true")]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert!(!parameters.contains_key(DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_partial_null_values_excludes_only_null_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, TEST_WAREHOUSE),
        (SCHEMA, TEST_SCHEMA),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert_eq!(
        Some(quoted(TEST_WAREHOUSE).as_str()),
        get(&parameters, DEFAULT_WAREHOUSE)
    );
    assert!(!parameters.contains_key(DB));
    assert_eq!(
        Some(quoted(TEST_SCHEMA).as_str()),
        get(&parameters, DEFAULT_SCHEMA)
    );
}

#[test]
fn connection_properties_to_environment_with_empty_hash_map_returns_environment_with_null_host() {
    let environment = to_environment(&HashMap::new());

    assert!(!environment.contains_key(WAREHOUSE));
    assert!(!environment.contains_key(DATABASE));
    assert!(!environment.contains_key(SCHEMA));
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}null");
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn connection_properties_to_environment_with_empty_string_values_returns_environment_with_empty_strings(
) {
    let properties = map(&[
        (WAREHOUSE, ""),
        (DATABASE, ""),
        (SCHEMA, ""),
        (HOST, ""),
        (PORT, ""),
    ]);

    let environment = to_environment(&properties);

    assert_eq!(Some(""), get(&environment, WAREHOUSE));
    assert_eq!(Some(""), get(&environment, DATABASE));
    assert_eq!(Some(""), get(&environment, SCHEMA));
    assert_eq!(Some(SNOWFLAKE_JDBC_SNOWFLAKE), get(&environment, DEFAULT));
}

#[test]
fn connection_properties_to_environment_with_special_characters_returns_environment_with_special_characters(
) {
    let special_host = "test-host@example.com";
    let special_warehouse = "warehouse#123";
    let special_database = "database$test";
    let special_schema = "schema%name";
    let special_port = "8080";
    let properties = map(&[
        (WAREHOUSE, special_warehouse),
        (DATABASE, special_database),
        (SCHEMA, special_schema),
        (HOST, special_host),
        (PORT, special_port),
    ]);

    let environment = to_environment(&properties);

    assert_eq!(Some(special_warehouse), get(&environment, WAREHOUSE));
    assert_eq!(Some(special_database), get(&environment, DATABASE));
    assert_eq!(Some(special_schema), get(&environment, SCHEMA));
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{special_host}:{special_port}");
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn connection_properties_to_environment_with_case_sensitivity_preserves_case() {
    let env_props = SnowflakeEnvironmentProperties::new();
    let mut properties = map(&[
        (WAREHOUSE, "TEST_WAREHOUSE"),
        (DATABASE, "TestDatabase"),
        (SCHEMA, "TEST_SCHEMA"),
        (HOST, TEST_HOST),
    ]);
    let expected = format!("{SNOWFLAKE_JDBC_SNOWFLAKE}{TEST_HOST}");

    let environment = env_props.connection_properties_to_environment(properties.clone());

    assert_eq!(Some("TEST_WAREHOUSE"), get(&environment, WAREHOUSE));
    assert_eq!(Some("TestDatabase"), get(&environment, DATABASE));
    assert_eq!(Some("TEST_SCHEMA"), get(&environment, SCHEMA));
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));

    properties.insert(WAREHOUSE.to_string(), "test_warehouse".to_string());
    properties.insert(SCHEMA.to_string(), "test_schema".to_string());
    let environment = env_props.connection_properties_to_environment(properties);

    assert_eq!(Some("test_warehouse"), get(&environment, WAREHOUSE));
    assert_eq!(Some("test_schema"), get(&environment, SCHEMA));
    assert_eq!(Some(expected.as_str()), get(&environment, DEFAULT));
}

#[test]
fn get_snow_flake_parameter_with_empty_hash_map_returns_empty_parameters() {
    let parameters =
        SnowflakeEnvironmentProperties::get_snow_flake_parameter(&HashMap::new(), &HashMap::new());
    assert!(parameters.is_empty());
}

#[test]
fn get_snow_flake_parameter_with_empty_string_values_excludes_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, ""),
        (DATABASE, ""),
        (SCHEMA, ""),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert!(!parameters.contains_key(DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_null_literal_values_excludes_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "null"),
        (DATABASE, "null"),
        (SCHEMA, "null"),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert!(!parameters.contains_key(DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_null_literal_mixed_case_and_whitespace_excludes_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "NULL"),
        (DATABASE, " Null "),
        (SCHEMA, "   "),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert!(!parameters.contains_key(DB));
    assert!(!parameters.contains_key(DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_null_literal_warehouse_only_keeps_other_parameters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "null"),
        (DATABASE, TEST_DATABASE),
        (SCHEMA, TEST_SCHEMA),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert!(!parameters.contains_key(DEFAULT_WAREHOUSE));
    assert_eq!(Some(quoted(TEST_DATABASE).as_str()), get(&parameters, DB));
    assert_eq!(
        Some(quoted(TEST_SCHEMA).as_str()),
        get(&parameters, DEFAULT_SCHEMA)
    );
}

#[test]
fn get_snow_flake_parameter_with_value_containing_null_is_preserved() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "NULLABLE_WH"),
        (DATABASE, "nullsafe_db"),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some("\"NULLABLE_WH\""), get(&parameters, DEFAULT_WAREHOUSE));
    assert_eq!(Some("\"nullsafe_db\""), get(&parameters, DB));
}

#[test]
fn get_snow_flake_parameter_with_special_characters_returns_quoted_special_characters() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "warehouse@123"),
        (DATABASE, "database#test"),
        (SCHEMA, "schema$name"),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert_eq!(
        Some("\"warehouse@123\""),
        get(&parameters, DEFAULT_WAREHOUSE)
    );
    assert_eq!(Some("\"database#test\""), get(&parameters, DB));
    assert_eq!(Some("\"schema$name\""), get(&parameters, DEFAULT_SCHEMA));
}

#[test]
fn get_snow_flake_parameter_with_case_sensitivity_preserves_case() {
    let connection_properties = map(&[
        (GLUE, "true"),
        (WAREHOUSE, "TEST_WAREHOUSE"),
        (DATABASE, "TestDatabase"),
        (SCHEMA, "test_schema"),
    ]);

    let parameters = SnowflakeEnvironmentProperties::get_snow_flake_parameter(
        &base_property(),
        &connection_properties,
    );

    assert_eq!(Some(TEST_USER), get(&parameters, USER));
    assert_eq!(
        Some("\"TEST_WAREHOUSE\""),
        get(&parameters, DEFAULT_WAREHOUSE)
    );
    assert_eq!(Some("\"TestDatabase\""), get(&parameters, DB));
    assert_eq!(Some("\"test_schema\""), get(&parameters, DEFAULT_SCHEMA));
}
