mod handler_support;

use std::collections::HashMap;
use std::sync::Arc;

use athena_federation_sdk::handlers::HandlerContext;
use athena_federation_sdk::request::{FederationRequest, PingRequest};
use athena_federation_sdk::security::{AwsRequestOverrideConfiguration, FederatedIdentity};
use aws_credential_types::Credentials;
use handler_support::{fas_config_options, FakeSecretsManager, Fakes, FAS_ACCESS};

const FAS_TOKEN: &str = "fas_token";
const SERVICE_KMS: &str = "service_kms_key_id";
const TOKEN: &str = "dGVzdC10b2tlbg==";

fn handler_context(fakes: &Fakes) -> HandlerContext {
    HandlerContext::new("TestType", fakes.clients(), HashMap::new()).unwrap()
}

struct Setup {
    fakes: Fakes,
    handler: HandlerContext,
}

fn set_up() -> Setup {
    set_up_with_secrets(FakeSecretsManager::default())
}

fn set_up_with_secrets(secrets: FakeSecretsManager) -> Setup {
    let fakes = Fakes::with_secrets(secrets);
    let handler = handler_context(&fakes);
    Setup { fakes, handler }
}

fn options(entries: &[(&str, &str)]) -> HashMap<String, Option<String>> {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), Some((*v).to_string())))
        .collect()
}

fn request_with(config_options: Option<HashMap<String, Option<String>>>) -> FederationRequest {
    let identity = config_options.map(|options| {
        FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), options)
    });
    FederationRequest::Ping(PingRequest::new(identity, "catalog", "queryId"))
}

fn override_with_credentials() -> AwsRequestOverrideConfiguration {
    AwsRequestOverrideConfiguration::builder()
        .credentials_provider(Credentials::new("a", "b", None, None, "test"))
        .build()
}

#[tokio::test]
async fn test_get_session_credentials() {
    let s = set_up();
    let result = s
        .handler
        .session_credentials("test-key-id", TOKEN)
        .await
        .unwrap();
    assert_eq!(
        Credentials::new(
            FAS_ACCESS,
            "fas-secret",
            Some("fas-session".to_string()),
            None,
            "FASToken"
        ),
        result
    );
    assert_eq!(Some("fas-session"), result.session_token());
    assert_eq!("fas-secret", result.secret_access_key());
    assert_eq!(None, result.expiry());
    let calls = s.fakes.crypto.calls.lock().unwrap().clone();
    assert_eq!(
        vec![("test-key-id".to_string(), b"test-token".to_vec())],
        calls
    );
}

#[tokio::test]
async fn test_get_request_override_config_with_valid_config() {
    let s = set_up();
    let config_options = fas_config_options();
    let result = s
        .handler
        .request_override_config(Some(&config_options))
        .await
        .unwrap();
    assert!(result.unwrap().credentials().is_some());
}

#[tokio::test]
async fn test_get_request_override_config_with_empty_config() {
    let s = set_up();
    let result = s
        .handler
        .request_override_config(Some(&HashMap::new()))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_request_override_config_with_null_config() {
    let s = set_up();
    let result = s.handler.request_override_config(None).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_request_override_config_with_missing_fas_token() {
    let s = set_up();
    let config_options = options(&[(SERVICE_KMS, "test-kms-key")]);
    let result = s
        .handler
        .request_override_config(Some(&config_options))
        .await
        .unwrap();
    assert!(result.is_none());
    assert!(s.fakes.crypto.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_get_request_override_config_with_missing_service_kms() {
    let s = set_up();
    let config_options = options(&[(FAS_TOKEN, TOKEN)]);
    let result = s
        .handler
        .request_override_config(Some(&config_options))
        .await
        .unwrap();
    assert!(result.is_none());
    assert!(s.fakes.crypto.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_get_request_override_config_map_overload() {
    let s = set_up();
    let config_options = fas_config_options();
    let result = s
        .handler
        .request_override_config(Some(&config_options))
        .await
        .unwrap();
    assert!(result.unwrap().credentials().is_some());
}

#[tokio::test]
async fn test_get_request_override_config_from_federation_request_with_federated_identity() {
    let s = set_up();
    let request = request_with(Some(fas_config_options()));
    let result = s.handler.federated_override_config(&request).await.unwrap();
    assert!(result.unwrap().credentials().is_some());
}

#[tokio::test]
async fn test_get_request_override_config_from_federation_request_with_null_identity() {
    let s = set_up();
    let result = s
        .handler
        .federated_override_config(&request_with(None))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_request_override_config_from_federation_request_with_null_config_options() {
    let s = set_up();
    let result = s
        .handler
        .federated_override_config(&request_with(Some(HashMap::new())))
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_request_override_config_from_federation_request_without_fas_token() {
    let s = set_up();
    let request = request_with(Some(options(&[(SERVICE_KMS, "test-kms-key")])));
    let result = s.handler.federated_override_config(&request).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_request_override_config_from_federation_request_non_federated() {
    let s = set_up();
    let request = request_with(Some(HashMap::new()));
    let result = s.handler.federated_override_config(&request).await.unwrap();
    assert!(result.is_none());
}

#[test]
fn test_get_s3_client_with_override_config() {
    let s = set_up();
    let result = s.handler.s3_client(Some(&override_with_credentials()));
    assert!(!Arc::ptr_eq(s.handler.default_s3(), &result));
}

#[test]
fn test_get_s3_client_with_null_override_config() {
    let s = set_up();
    let result = s.handler.s3_client(None);
    assert!(Arc::ptr_eq(s.handler.default_s3(), &result));
}

#[test]
fn test_get_s3_client_with_override_config_but_no_credentials() {
    let s = set_up();
    let override_config = AwsRequestOverrideConfiguration::builder().build();
    let result = s.handler.s3_client(Some(&override_config));
    assert!(Arc::ptr_eq(s.handler.default_s3(), &result));
}

#[test]
fn test_get_athena_client_with_override_config() {
    let s = set_up();
    let result = s.handler.athena_client(Some(&override_with_credentials()));
    assert!(!Arc::ptr_eq(s.handler.default_athena(), &result));
}

#[test]
fn test_get_athena_client_with_null_override_config() {
    let s = set_up();
    let result = s.handler.athena_client(None);
    assert!(Arc::ptr_eq(s.handler.default_athena(), &result));
}

#[test]
fn test_get_athena_client_with_override_config_but_no_credentials() {
    let s = set_up();
    let override_config = AwsRequestOverrideConfiguration::builder().build();
    let result = s.handler.athena_client(Some(&override_config));
    assert!(Arc::ptr_eq(s.handler.default_athena(), &result));
}

#[test]
fn test_is_request_federated_with_federated_request() {
    let request = request_with(Some(options(&[(FAS_TOKEN, "test-token")])));
    assert!(request.is_federated());
}

#[test]
fn test_is_request_federated_with_non_federated_request() {
    assert!(!request_with(Some(HashMap::new())).is_federated());
}

#[test]
fn test_is_request_federated_with_null_identity() {
    assert!(!request_with(None).is_federated());
}

#[test]
fn test_is_request_federated_with_null_config_options() {
    let request = request_with(Some(HashMap::from([(FAS_TOKEN.to_string(), None)])));
    assert!(!request.is_federated());
}

#[tokio::test]
async fn test_resolve_secrets() {
    let s = set_up_with_secrets(FakeSecretsManager::with_secret(
        "WithSecret",
        "ResolvedSecret",
    ));
    let result = s
        .handler
        .secrets_manager()
        .resolve_secrets(Some("MyString${WithSecret}"))
        .await
        .unwrap();
    assert_eq!(Some("MyStringResolvedSecret".to_string()), result);
    assert_eq!(
        vec![("WithSecret".to_string(), None)],
        s.fakes.secrets.calls()
    );
}

#[tokio::test]
async fn test_resolve_with_default_credentials() {
    let s = set_up_with_secrets(FakeSecretsManager::with_secret(
        "Secret",
        "{\"username\":\"username\",\"password\":\"password\"}",
    ));
    let result = s
        .handler
        .secrets_manager()
        .resolve_with_default_credentials(Some("MyString${Secret}"))
        .await
        .unwrap();
    assert_eq!(Some("MyStringusername:password".to_string()), result);
    assert_eq!("Secret", s.fakes.secrets.calls()[0].0);
}

#[tokio::test]
async fn test_get_secret() {
    let s = set_up_with_secrets(FakeSecretsManager::with_secret(
        "test-secret",
        "secret-value",
    ));
    let result = s.handler.secret("test-secret", None).await.unwrap();
    assert_eq!("secret-value", result);
    assert_eq!(
        vec![("test-secret".to_string(), None)],
        s.fakes.secrets.calls()
    );
}

#[tokio::test]
async fn test_get_secret_with_request_override_configuration() {
    let s = set_up_with_secrets(FakeSecretsManager::with_secret(
        "test-secret",
        "secret-value",
    ));
    let override_config = override_with_credentials();
    let result = s
        .handler
        .secret("test-secret", Some(&override_config))
        .await
        .unwrap();
    assert_eq!("secret-value", result);
    assert_eq!(
        vec![("test-secret".to_string(), Some(override_config))],
        s.fakes.secrets.calls()
    );
}

#[tokio::test]
async fn test_get_credential_provider_with_valid_secret_name() {
    let secret_value = "{\"username\":\"testuser\",\"password\":\"testpass\"}";
    let fakes = Fakes::with_secrets(FakeSecretsManager::with_secret(
        "test-db-secret",
        secret_value,
    ));
    let handler_with_secret = handler_context(&fakes);
    let override_config = override_with_credentials();
    let result = handler_with_secret
        .credentials_provider(Some("test-db-secret"), Some(&override_config))
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(
        vec![("test-db-secret".to_string(), Some(override_config))],
        fakes.secrets.calls()
    );
}

#[tokio::test]
async fn test_get_credential_provider_with_blank_secret_name() {
    let fakes = Fakes::new();
    let handler_without_secret = handler_context(&fakes);
    let result = handler_without_secret
        .credentials_provider(Some(""), None)
        .await
        .unwrap();
    assert!(result.is_none());
    assert!(fakes.secrets.calls().is_empty());
}

#[tokio::test]
async fn test_get_credential_provider_with_null_secret_name() {
    let s = set_up();
    let result = s.handler.credentials_provider(None, None).await.unwrap();
    assert!(result.is_none());
    assert!(s.fakes.secrets.calls().is_empty());
}

#[tokio::test]
async fn test_create_credentials_provider() {
    let secret_value = "{\"username\":\"testuser\",\"password\":\"testpass\"}";
    let s = set_up_with_secrets(FakeSecretsManager::with_secret("test-secret", secret_value));
    let override_config = override_with_credentials();
    let result = s
        .handler
        .credentials_provider(Some("test-secret"), Some(&override_config))
        .await
        .unwrap()
        .unwrap();
    let credential = result.get_credential().await.unwrap();
    assert_eq!("testuser", credential.properties()["user"]);
    assert_eq!(
        vec![("test-secret".to_string(), Some(override_config))],
        s.fakes.secrets.calls()
    );
}

#[tokio::test]
async fn test_get_database_connection_secret_default_behavior() {
    let s = set_up();
    let result = s.handler.credentials_provider(None, None).await.unwrap();
    assert!(result.is_none());
}
