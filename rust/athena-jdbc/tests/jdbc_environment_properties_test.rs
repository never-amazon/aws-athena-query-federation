use std::collections::HashMap;

use athena_federation_sdk::connection::{environment_constants, EnvironmentProperties};
use athena_jdbc::JdbcEnvironmentProperties;

const TEST_HOST: &str = "test.host.com";
const TEST_PORT: &str = "1234";
const TEST_DATABASE: &str = "testdb";
const TEST_SECRET: &str = "testSecret";
const TEST_ENCRYPT_PARAM: &str = "encrypt=false";
const CONNECTION_STRING_PREFIX: &str = "databaseName://jdbc:databaseName://";

struct TestEnvironmentProperties {
    environment_properties: EnvironmentProperties,
}

impl JdbcEnvironmentProperties for TestEnvironmentProperties {
    fn environment_properties(&self) -> &EnvironmentProperties {
        &self.environment_properties
    }

    fn get_connection_string_prefix(
        &self,
        _connection_properties: &HashMap<String, String>,
    ) -> String {
        CONNECTION_STRING_PREFIX.to_string()
    }
}

fn base_connection_string() -> String {
    format!("{CONNECTION_STRING_PREFIX}{TEST_HOST}:{TEST_PORT}/{TEST_DATABASE}")
}

fn setup() -> (HashMap<String, String>, TestEnvironmentProperties) {
    let connection_properties = HashMap::from([
        (
            environment_constants::HOST.to_string(),
            TEST_HOST.to_string(),
        ),
        (
            environment_constants::PORT.to_string(),
            TEST_PORT.to_string(),
        ),
        (
            environment_constants::DATABASE.to_string(),
            TEST_DATABASE.to_string(),
        ),
    ]);
    let properties = TestEnvironmentProperties {
        environment_properties: EnvironmentProperties::new(),
    };
    (connection_properties, properties)
}

fn default_of(result: &HashMap<String, String>) -> &str {
    result.get(environment_constants::DEFAULT).unwrap()
}

#[test]
fn test_connection_properties_with_no_params() {
    let (connection_properties, properties) = setup();
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{}?", base_connection_string()),
        default_of(&result)
    );
}

#[test]
fn test_connection_properties_with_only_secret() {
    let (mut connection_properties, properties) = setup();
    connection_properties.insert(
        environment_constants::SECRET_NAME.to_string(),
        TEST_SECRET.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{}?${{{TEST_SECRET}}}", base_connection_string()),
        default_of(&result)
    );
}

#[test]
fn test_connection_properties_with_only_jdbc_params() {
    let (mut connection_properties, properties) = setup();
    let ssl_param = "ssl=true";
    connection_properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        ssl_param.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{}?{ssl_param}", base_connection_string()),
        default_of(&result)
    );
}

#[test]
fn test_connection_properties_with_database_and_params_and_secret() {
    let (mut connection_properties, properties) = setup();
    connection_properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        TEST_ENCRYPT_PARAM.to_string(),
    );
    connection_properties.insert(
        environment_constants::SECRET_NAME.to_string(),
        TEST_SECRET.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!(
            "{}?{TEST_ENCRYPT_PARAM}&${{{TEST_SECRET}}}",
            base_connection_string()
        ),
        default_of(&result)
    );
}

#[test]
fn test_special_characters_in_connection_params() {
    let (mut connection_properties, properties) = setup();
    let special_host = "test.host-with_special.chars.com";
    let special_db = "test/db#1";
    let special_params = "param1=value1&param2=value2#hash;param3=value3";
    connection_properties.insert(
        environment_constants::HOST.to_string(),
        special_host.to_string(),
    );
    connection_properties.insert(
        environment_constants::DATABASE.to_string(),
        special_db.to_string(),
    );
    connection_properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        special_params.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!(
            "{CONNECTION_STRING_PREFIX}{special_host}:{TEST_PORT}/{special_db}?{special_params}"
        ),
        default_of(&result)
    );
}

#[test]
fn test_multiple_jdbc_parameters() {
    let (mut connection_properties, properties) = setup();
    let params = "ssl=true&connectTimeout=10000&socketTimeout=5000&autoReconnect=true&useUnicode=true&characterEncoding=UTF-8";
    connection_properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        params.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{}?{params}", base_connection_string()),
        default_of(&result)
    );
}

#[test]
fn test_complex_database_path() {
    let (mut connection_properties, properties) = setup();
    let complex_db = "main/schema1/table2";
    connection_properties.insert(
        environment_constants::DATABASE.to_string(),
        complex_db.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{CONNECTION_STRING_PREFIX}{TEST_HOST}:{TEST_PORT}/{complex_db}?"),
        default_of(&result)
    );
}

#[test]
fn test_secret_with_special_characters() {
    let (mut connection_properties, properties) = setup();
    let special_secret = "secret/name#1";
    connection_properties.insert(
        environment_constants::SECRET_NAME.to_string(),
        special_secret.to_string(),
    );
    let result = properties.connection_properties_to_environment(connection_properties);
    assert_eq!(
        format!("{}?${{{special_secret}}}", base_connection_string()),
        default_of(&result)
    );
}
