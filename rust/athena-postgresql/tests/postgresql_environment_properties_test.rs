use std::collections::HashMap;

use athena_federation_sdk::connection::EnvironmentConstants;
use athena_jdbc::JdbcEnvironmentProperties;
use athena_postgresql::PostGreSqlEnvironmentProperties;

fn connection_properties() -> HashMap<String, String> {
    HashMap::from([
        (
            EnvironmentConstants::HOST.to_string(),
            "postgres-endpoint".to_string(),
        ),
        (
            EnvironmentConstants::DATABASE.to_string(),
            "testdb".to_string(),
        ),
        (
            EnvironmentConstants::SECRET_NAME.to_string(),
            "postgres-secret".to_string(),
        ),
        (EnvironmentConstants::PORT.to_string(), "5000".to_string()),
    ])
}

fn connection_string(properties: HashMap<String, String>) -> String {
    PostGreSqlEnvironmentProperties::new()
        .connection_properties_to_environment(properties)
        .remove(EnvironmentConstants::DEFAULT)
        .unwrap()
}

#[test]
fn test_basic_connection_string() {
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?${postgres-secret}",
        connection_string(connection_properties())
    );
}

#[test]
fn test_connection_string_with_jdbc_params() {
    let mut properties = connection_properties();
    properties.insert(
        EnvironmentConstants::JDBC_PARAMS.to_string(),
        "ssl=true".to_string(),
    );
    properties.remove(EnvironmentConstants::SECRET_NAME);
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true",
        connection_string(properties)
    );
}

#[test]
fn test_connection_string_with_jdbc_params_and_secret() {
    let mut properties = connection_properties();
    properties.insert(
        EnvironmentConstants::JDBC_PARAMS.to_string(),
        "ssl=true".to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true&${postgres-secret}",
        connection_string(properties)
    );
}

#[test]
fn test_connection_string_without_optional_params() {
    let mut properties = connection_properties();
    properties.remove(EnvironmentConstants::SECRET_NAME);
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?",
        connection_string(properties)
    );
}

#[test]
fn test_connection_string_with_empty_values() {
    let mut properties = connection_properties();
    properties.insert(EnvironmentConstants::JDBC_PARAMS.to_string(), String::new());
    properties.insert(EnvironmentConstants::SECRET_NAME.to_string(), String::new());
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?&${}",
        connection_string(properties)
    );
}

#[test]
fn test_empty_jdbc_params_with_secret() {
    let mut properties = connection_properties();
    properties.insert(EnvironmentConstants::JDBC_PARAMS.to_string(), String::new());
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?&${postgres-secret}",
        connection_string(properties)
    );
}

#[test]
fn test_special_characters_in_parameters() {
    let mut properties = connection_properties();
    properties.insert(
        EnvironmentConstants::HOST.to_string(),
        "postgres.example.com".to_string(),
    );
    properties.insert(
        EnvironmentConstants::DATABASE.to_string(),
        "test/db".to_string(),
    );
    properties.insert(
        EnvironmentConstants::JDBC_PARAMS.to_string(),
        "currentSchema=public&search_path=public%2Cshared".to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres.example.com:5000/test/db?currentSchema=public&search_path=public%2Cshared&${postgres-secret}",
        connection_string(properties)
    );
}

#[test]
fn test_multiple_jdbc_parameters() {
    let mut properties = connection_properties();
    properties.insert(
        EnvironmentConstants::JDBC_PARAMS.to_string(),
        "ssl=true&sslmode=verify-full&connectTimeout=10&socketTimeout=20&tcpKeepAlive=true"
            .to_string(),
    );
    assert_eq!(
        "postgres://jdbc:postgresql://postgres-endpoint:5000/testdb?ssl=true&sslmode=verify-full&connectTimeout=10&socketTimeout=20&tcpKeepAlive=true&${postgres-secret}",
        connection_string(properties)
    );
}
