use athena_federation_sdk::exceptions::JavaThrowable;
use athena_jdbc::connection::{DatabaseConnectionConfig, DatabaseConnectionConfigBuilder};

const CONNECTION_STRING1: &str = "mysql://jdbc:mysql://hostname/${testSecret}";
const CONNECTION_STRING2: &str =
    "postgres://jdbc:postgresql://hostname/user=testUser&password=testPassword";
const CONNECTION_STRING3: &str = "redshift://jdbc:redshift://hostname:5439/dev?${arn:aws:secretsmanager:us-east-1:1234567890:secret:redshift/user/secret}";
const CONNECTION_STRING4: &str = "postgres://jdbc:postgresql://hostname:5439/dev?${arn:aws:secretsmanager:us-east-1:1234567890:secret:postgresql/user/secret}";

#[test]
fn build() {
    let default_connection = DatabaseConnectionConfig::new(
        "default",
        "postgres",
        "jdbc:postgresql://hostname/user=testUser&password=testPassword",
    )
    .unwrap();
    let expected_database1 = DatabaseConnectionConfig::new_with_secret(
        "testCatalog1",
        "postgres",
        "jdbc:postgresql://hostname:5439/dev?${arn:aws:secretsmanager:us-east-1:1234567890:secret:postgresql/user/secret}",
        "arn:aws:secretsmanager:us-east-1:1234567890:secret:postgresql/user/secret",
    )
    .unwrap();
    let expected_database2 = DatabaseConnectionConfig::new(
        "testCatalog2",
        "postgres",
        "jdbc:postgresql://hostname/user=testUser&password=testPassword",
    )
    .unwrap();

    let configs = DatabaseConnectionConfigBuilder::new()
        .engine("postgres")
        .properties([
            ("default", CONNECTION_STRING2),
            ("testCatalog1_connection_string", CONNECTION_STRING4),
            ("testCatalog2_connection_string", CONNECTION_STRING2),
        ])
        .build()
        .unwrap();

    assert_eq!(
        vec![default_connection, expected_database1, expected_database2],
        configs
    );
}

#[test]
fn build_multiple_databases_fails() {
    let result = DatabaseConnectionConfigBuilder::new()
        .properties([
            ("default", CONNECTION_STRING2),
            ("testCatalog1_connection_string", CONNECTION_STRING1),
            ("testCatalog2_connection_string", CONNECTION_STRING2),
            ("testCatalog3_connection_string", CONNECTION_STRING3),
        ])
        .build();
    assert!(result.unwrap_err().is_runtime_exception());
}

#[test]
fn build_invalid_connection_string() {
    let result = DatabaseConnectionConfigBuilder::new()
        .properties([("default", "malformedUrl")])
        .build();
    assert!(result.unwrap_err().is_runtime_exception());
}

#[test]
fn build_with_no_default() {
    let result = DatabaseConnectionConfigBuilder::new()
        .properties([("testDb_connection_string", CONNECTION_STRING1)])
        .build();
    assert!(result.unwrap_err().is_runtime_exception());
}

#[test]
fn build_malformed_connection_string() {
    let result = DatabaseConnectionConfigBuilder::new()
        .properties([("testDb_connection_string", "")])
        .build();
    assert!(result.unwrap_err().is_runtime_exception());
}

#[test]
fn invalid_secrets_syntax_test() {
    let engine = "redshift";
    let invalid_connection_strings = [
        "redshift://jdbc:redshift://hostname:5439/dev?${inv&li$dSecret}",
        "redshift://jdbc:redshift://hostname:5439/dev?${an*therOne))}",
        "redshift://jdbc:redshift://hostname:5439/dev?${in^a]i?}",
        "redshift://jdbc:redshift://hostname:5439/dev?${an*therOne))}",
    ];
    for connection in invalid_connection_strings {
        let result = DatabaseConnectionConfigBuilder::new()
            .properties([("testDb_connection_string", connection)])
            .engine(engine)
            .build();
        assert!(result.unwrap_err().is_runtime_exception());
    }
}

#[test]
fn valid_secrets_syntax_test() {
    let engine = "redshift";
    let secrets = [
        "spec.@/Ch@rac+=r_",
        "rds!service-linked-secret",
        "redshift:credentials-secret",
        "opsworks-cm:credentials12",
    ];
    let configs = DatabaseConnectionConfigBuilder::new()
        .engine(engine)
        .properties([
            (
                "default",
                "redshift://jdbc:redshift://hostname:5439/dev?${spec.@/Ch@rac+=r_}",
            ),
            (
                "testCatalog2_connection_string",
                "redshift://jdbc:redshift://hostname:5439/dev?${rds!service-linked-secret}",
            ),
            (
                "testCatalog3_connection_string",
                "redshift://jdbc:redshift://hostname:5439/dev?${redshift:credentials-secret}",
            ),
            (
                "testCatalog4_connection_string",
                "redshift://jdbc:redshift://hostname:5439/dev?${opsworks-cm:credentials12}",
            ),
        ])
        .build()
        .unwrap();
    assert_eq!(secrets.len(), configs.len());
    for (config, secret) in configs.iter().zip(secrets) {
        assert_eq!(Some(secret), config.secret());
    }
}
