use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use athena_federation_sdk::credentials::{
    CredentialsConstants, CredentialsProviderFactory, DefaultCredentialsProvider, HttpRequest,
    OAuthCredentialsProvider, OAuthFlow,
};
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, CachableSecretsManager, SecretsManagerClient,
};
use athena_federation_sdk::Result;
use athena_federation_sdk::{exceptions::FederationSourceErrorCode, FederationError};

const SECRET_NAME: &str = "test-secret";
const TEST_CLIENT_ID: &str = "test-client-id";
const TEST_CLIENT_SECRET: &str = "test-client-secret";
const TEST_USERNAME: &str = "test-user";
const TEST_PASSWORD: &str = "test-password";

#[derive(Default)]
struct MockSecretsManager {
    secret_string: Mutex<Option<String>>,
}

#[async_trait]
impl SecretsManagerClient for MockSecretsManager {
    async fn get_secret_value(
        &self,
        _secret_id: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<Option<String>, AwsServiceError> {
        Ok(self.secret_string.lock().unwrap().clone())
    }

    async fn put_secret_value(
        &self,
        _secret_id: &str,
        _secret_string: &str,
    ) -> std::result::Result<(), AwsServiceError> {
        Ok(())
    }
}

struct TestOAuthProvider;

impl OAuthFlow for TestOAuthProvider {
    fn is_oauth_configured(&self, secret_map: &HashMap<String, String>) -> Result<bool> {
        Ok(secret_map.contains_key(CredentialsConstants::CLIENT_ID)
            && secret_map.contains_key(CredentialsConstants::CLIENT_SECRET))
    }

    fn build_token_request(&self, _secret_map: &HashMap<String, String>) -> Result<HttpRequest> {
        Ok(HttpRequest::builder("http://test").build())
    }
}

struct InvalidOAuthProvider;

impl OAuthFlow for InvalidOAuthProvider {
    fn is_oauth_configured(&self, _secret_map: &HashMap<String, String>) -> Result<bool> {
        Err(FederationError::Runtime("Test exception".into()))
    }

    fn build_token_request(&self, _secret_map: &HashMap<String, String>) -> Result<HttpRequest> {
        Err(FederationError::NullPointer("no request".into()))
    }
}

fn setup(secret: Option<String>) -> Arc<CachableSecretsManager> {
    let secrets_manager = Arc::new(MockSecretsManager {
        secret_string: Mutex::new(secret),
    });
    Arc::new(CachableSecretsManager::new(secrets_manager))
}

fn oauth_secret() -> String {
    serde_json::to_string(&HashMap::from([
        (CredentialsConstants::CLIENT_ID, TEST_CLIENT_ID),
        (CredentialsConstants::CLIENT_SECRET, TEST_CLIENT_SECRET),
    ]))
    .unwrap()
}

fn test_oauth_provider(
) -> Box<dyn athena_federation_sdk::credentials::InitializableCredentialsProvider> {
    Box::new(OAuthCredentialsProvider::new(Box::new(TestOAuthProvider)))
}

#[tokio::test]
async fn test_create_credential_provider_when_oauth_configured() {
    let cachable_secrets_manager = setup(Some(oauth_secret()));
    let provider = CredentialsProviderFactory::create_credential_provider(
        Some(SECRET_NAME),
        &cachable_secrets_manager,
        Some(test_oauth_provider()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(provider.as_any().is::<OAuthCredentialsProvider>());
}

#[tokio::test]
async fn test_create_credential_provider_when_username_password_configured() {
    let standard_secret = serde_json::to_string(&HashMap::from([
        (CredentialsConstants::USERNAME, TEST_USERNAME),
        (CredentialsConstants::PASSWORD, TEST_PASSWORD),
    ]))
    .unwrap();
    let cachable_secrets_manager = setup(Some(standard_secret));
    let provider = CredentialsProviderFactory::create_credential_provider(
        Some(SECRET_NAME),
        &cachable_secrets_manager,
        Some(test_oauth_provider()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(provider.as_any().is::<DefaultCredentialsProvider>());
}

#[tokio::test]
async fn test_create_credential_provider_when_no_secret() {
    let cachable_secrets_manager = setup(None);
    let provider = CredentialsProviderFactory::create_credential_provider(
        Some(""),
        &cachable_secrets_manager,
        Some(test_oauth_provider()),
    )
    .await
    .unwrap();
    assert!(provider.is_none());
}

#[tokio::test]
async fn test_create_credential_provider_when_invalid_json_throws_exception() {
    let cachable_secrets_manager = setup(Some("invalid-json{".to_string()));
    let e = CredentialsProviderFactory::create_credential_provider(
        Some(SECRET_NAME),
        &cachable_secrets_manager,
        Some(test_oauth_provider()),
    )
    .await
    .err()
    .expect("Expected AthenaConnectorException");
    let connector = e
        .as_connector_exception()
        .expect("Expected AthenaConnectorException");
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException.as_str()),
        connector.get_error_details().error_code()
    );
    assert!(e
        .to_string()
        .contains("Could not deserialize credentials into HashMap"));
}

#[tokio::test]
async fn test_create_credential_provider_when_invalid_oauth_class_throws_exception() {
    let cachable_secrets_manager = setup(Some(oauth_secret()));
    let e = CredentialsProviderFactory::create_credential_provider(
        Some(SECRET_NAME),
        &cachable_secrets_manager,
        Some(Box::new(OAuthCredentialsProvider::new(Box::new(
            InvalidOAuthProvider,
        )))),
    )
    .await
    .err()
    .expect("Expected AthenaConnectorException");
    let connector = e
        .as_connector_exception()
        .expect("Expected AthenaConnectorException");
    assert_eq!(
        Some(FederationSourceErrorCode::InternalServiceException.as_str()),
        connector.get_error_details().error_code()
    );
    assert!(e.to_string().contains("Failed to create OAuth provider"));
}
