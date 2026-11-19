use std::collections::HashMap;

use athena_federation_sdk::connection::environment_constants;
use athena_postgresql::connection_properties_to_environment;

fn connection_properties() -> HashMap<String, String> {
    HashMap::from([
        (
            environment_constants::HOST.to_string(),
            "postgres-endpoint".to_string(),
        ),
        (
            environment_constants::DATABASE.to_string(),
            "testdb".to_string(),
        ),
        (
            environment_constants::SECRET_NAME.to_string(),
            "postgres-secret".to_string(),
        ),
        (environment_constants::PORT.to_string(), "5000".to_string()),
    ])
}

fn connection_string(properties: &HashMap<String, String>) -> String {
    connection_properties_to_environment(properties)
        .remove(environment_constants::DEFAULT)
        .unwrap()
}

#[test]
fn test_basic_connection_string() {
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?${postgres-secret}",
        connection_string(&connection_properties())
    );
}

#[test]
fn test_connection_string_with_jdbc_params() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        "ssl=true".to_string(),
    );
    properties.remove(environment_constants::SECRET_NAME);
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true",
        connection_string(&properties)
    );
}

#[test]
fn test_connection_string_with_jdbc_params_and_secret() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        "ssl=true".to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true&${postgres-secret}",
        connection_string(&properties)
    );
}

#[test]
fn test_connection_string_without_optional_params() {
    let mut properties = connection_properties();
    properties.remove(environment_constants::SECRET_NAME);
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?",
        connection_string(&properties)
    );
}

#[test]
fn test_connection_string_with_empty_values() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        String::new(),
    );
    properties.insert(
        environment_constants::SECRET_NAME.to_string(),
        String::new(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?&${}",
        connection_string(&properties)
    );
}

#[test]
fn test_empty_jdbc_params_with_secret() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        String::new(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?&${postgres-secret}",
        connection_string(&properties)
    );
}

#[test]
fn test_special_characters_in_parameters() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::HOST.to_string(),
        "postgres.example.com".to_string(),
    );
    properties.insert(
        environment_constants::DATABASE.to_string(),
        "test/db".to_string(),
    );
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        "currentSchema=public&search_path=public%2Cshared".to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres.example.com:5000/test/db?currentSchema=public&search_path=public%2Cshared&${postgres-secret}",
        connection_string(&properties)
    );
}

#[test]
fn test_multiple_jdbc_parameters() {
    let mut properties = connection_properties();
    properties.insert(
        environment_constants::JDBC_PARAMS.to_string(),
        "ssl=true&sslmode=verify-full&connectTimeout=10&socketTimeout=20&tcpKeepAlive=true"
            .to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true&sslmode=verify-full&connectTimeout=10&socketTimeout=20&tcpKeepAlive=true&${postgres-secret}",
        connection_string(&properties)
    );
}
