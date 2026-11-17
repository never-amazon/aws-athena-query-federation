use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use athena_federation_sdk::connection::{EnvironmentConstants, EnvironmentProperties, GlueConnectionClient};
use athena_federation_sdk::security::AwsServiceError;
use aws_sdk_glue::types::{AuthenticationConfiguration, Connection, ConnectionPropertyKey};

const GLUE_CONN_NAME: &str = "my-glue-conn";
const SECRET_ARN: &str = "arn:aws:secretsmanager:us-east-1:1234567890:secret:my-secret-abc123";
const EXPECTED_SECRET_NAME: &str = "my-secret";
const TEST_VALUE: &str = "test";
const KMS_KEY_ID: &str = "kms-123";
const LAMBDA_VALUE: &str = "lambda-value";

struct FakeGlue {
    connection: Result<Connection, AwsServiceError>,
}

#[async_trait]
impl GlueConnectionClient for FakeGlue {
    async fn get_connection(&self, name: &str) -> Result<Connection, AwsServiceError> {
        assert_eq!(GLUE_CONN_NAME, name);
        self.connection.clone()
    }
}

#[tokio::test]
async fn test_create_environment_with_overridden_env() {
    let fake_env = HashMap::from([
        (EnvironmentConstants::DEFAULT_GLUE_CONNECTION.to_string(), GLUE_CONN_NAME.to_string()),
        ("OVERRIDE_VAR".to_string(), LAMBDA_VALUE.to_string()),
    ]);
    let auth_config = AuthenticationConfiguration::builder().secret_arn(SECRET_ARN).build();
    let glue_connection = Connection::builder()
        .name(GLUE_CONN_NAME)
        .connection_properties(ConnectionPropertyKey::Database, TEST_VALUE)
        .authentication_configuration(auth_config)
        .athena_properties(EnvironmentConstants::SPILL_KMS_KEY_ID, KMS_KEY_ID)
        .build();
    let spy_props = EnvironmentProperties::new()
        .with_env_map(fake_env)
        .with_glue_client(Arc::new(FakeGlue {
            connection: Ok(glue_connection),
        }));

    let result = spy_props.create_environment().await.unwrap();

    assert_eq!(Some(GLUE_CONN_NAME), result.get(EnvironmentConstants::DEFAULT_GLUE_CONNECTION).map(String::as_str));
    assert_eq!(Some(TEST_VALUE), result.get(EnvironmentConstants::DATABASE).map(String::as_str));
    assert_eq!(Some(EXPECTED_SECRET_NAME), result.get(EnvironmentConstants::SECRET_NAME).map(String::as_str));
    assert_eq!(Some(KMS_KEY_ID), result.get(EnvironmentConstants::KMS_KEY_ID).map(String::as_str));
    assert_eq!(Some(LAMBDA_VALUE), result.get("OVERRIDE_VAR").map(String::as_str));
}

#[tokio::test]
async fn test_create_environment_with_overridden_env_glue_connection_fails_throws_runtime_exception() {
    let fake_env = HashMap::from([(
        EnvironmentConstants::DEFAULT_GLUE_CONNECTION.to_string(),
        GLUE_CONN_NAME.to_string(),
    )]);
    let spy_props = EnvironmentProperties::new()
        .with_env_map(fake_env)
        .with_glue_client(Arc::new(FakeGlue {
            connection: Err(AwsServiceError::new("Simulated failure")),
        }));

    let thrown = spy_props.create_environment().await.unwrap_err();
    assert!(thrown.is_runtime_exception());
    assert_eq!("Simulated failure", thrown.to_string());
}
