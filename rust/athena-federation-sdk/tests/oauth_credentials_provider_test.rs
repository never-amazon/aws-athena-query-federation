use athena_federation_sdk::credentials::credentials_constants;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use athena_federation_sdk::credentials::{
    CredentialsProvider, HttpClient, HttpClientError, HttpRequest, HttpResponse,
    OAuthCredentialsProvider, OAuthFlow,
};
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsService, AwsServiceError, CachableSecretsManager,
    SecretsManagerClient,
};
use athena_federation_sdk::Result;
use athena_federation_sdk::{exceptions::FederationSourceErrorCode, FederationError};

const TEST_SECRET_NAME: &str = "test-secret";
const TEST_CLIENT_ID: &str = "test-client-id";
const TEST_CLIENT_SECRET: &str = "test-client-secret";
const TEST_ACCESS_TOKEN: &str = "test-access-token";

#[derive(Default)]
struct MockSecretsClient {
    put_error: Mutex<Option<AwsServiceError>>,
}

#[async_trait]
impl SecretsManagerClient for MockSecretsClient {
    async fn get_secret_value(
        &self,
        _secret_id: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<Option<String>, AwsServiceError> {
        Ok(None)
    }

    async fn put_secret_value(
        &self,
        _secret_id: &str,
        _secret_string: &str,
        _override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> std::result::Result<(), AwsServiceError> {
        match self.put_error.lock().unwrap().clone() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

#[derive(Default)]
struct MockHttpClient {
    response: Mutex<Option<std::result::Result<HttpResponse, HttpClientError>>>,
}

#[async_trait]
impl HttpClient for MockHttpClient {
    async fn send(
        &self,
        _request: HttpRequest,
    ) -> std::result::Result<HttpResponse, HttpClientError> {
        self.response
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Err(HttpClientError::Io("no response configured".into())))
    }
}

struct TestOAuthFlow;

impl OAuthFlow for TestOAuthFlow {
    fn is_oauth_configured(&self, secret_map: &HashMap<String, String>) -> Result<bool> {
        Ok(secret_map
            .get(credentials_constants::CLIENT_ID)
            .is_some_and(|v| !v.is_empty())
            && secret_map
                .get(credentials_constants::CLIENT_SECRET)
                .is_some_and(|v| !v.is_empty()))
    }

    fn build_token_request(&self, _secret_map: &HashMap<String, String>) -> Result<HttpRequest> {
        Ok(HttpRequest::builder("http://test").build())
    }
}

struct Setup {
    mock_secrets_client: Arc<MockSecretsClient>,
    mock_http_client: Arc<MockHttpClient>,
    credentials_provider: OAuthCredentialsProvider,
    secrets_manager: Arc<CachableSecretsManager>,
}

impl Setup {
    fn mock_secret_response(&mut self, secret_json: &str) {
        if secret_json.contains(credentials_constants::CLIENT_ID)
            || secret_json.contains(credentials_constants::ACCESS_TOKEN)
        {
            let secret_map: HashMap<String, String> = serde_json::from_str(secret_json).unwrap();
            self.credentials_provider =
                oauth_provider(&self.mock_http_client, &self.secrets_manager, secret_map);
        }
    }

    fn mock_http_client_for_token_fetch(&self, response_body: &str) {
        self.mock_http_response(200, response_body);
    }

    fn mock_http_response(&self, status_code: u16, response_body: &str) {
        *self.mock_http_client.response.lock().unwrap() =
            Some(Ok(HttpResponse::new(status_code, response_body)));
    }

    fn mock_http_error(&self, error: HttpClientError) {
        *self.mock_http_client.response.lock().unwrap() = Some(Err(error));
    }

    fn mock_put_error(&self, error: AwsServiceError) {
        *self.mock_secrets_client.put_error.lock().unwrap() = Some(error);
    }
}

fn oauth_provider(
    http_client: &Arc<MockHttpClient>,
    secrets_manager: &Arc<CachableSecretsManager>,
    secret_map: HashMap<String, String>,
) -> OAuthCredentialsProvider {
    OAuthCredentialsProvider::new(
        Box::new(TestOAuthFlow),
        TEST_SECRET_NAME,
        secret_map,
        Arc::clone(secrets_manager),
        Arc::clone(http_client) as Arc<dyn HttpClient>,
    )
}

fn set_up() -> Setup {
    let mock_secrets_client = Arc::new(MockSecretsClient::default());
    let mock_http_client = Arc::new(MockHttpClient::default());
    let secrets_manager = Arc::new(CachableSecretsManager::new(mock_secrets_client.clone()));
    let credentials_provider = oauth_provider(&mock_http_client, &secrets_manager, HashMap::new());
    Setup {
        mock_secrets_client,
        mock_http_client,
        credentials_provider,
        secrets_manager,
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .cast_signed()
}

fn create_oauth_secret_json() -> String {
    serde_json::json!({
        credentials_constants::CLIENT_ID: TEST_CLIENT_ID,
        credentials_constants::CLIENT_SECRET: TEST_CLIENT_SECRET,
    })
    .to_string()
}

fn create_oauth_secret_json_with_token(access_token: &str, fetched_at: i64) -> String {
    serde_json::json!({
        credentials_constants::CLIENT_ID: TEST_CLIENT_ID,
        credentials_constants::CLIENT_SECRET: TEST_CLIENT_SECRET,
        credentials_constants::ACCESS_TOKEN: access_token,
        credentials_constants::EXPIRES_IN: "3600",
        credentials_constants::FETCHED_AT: fetched_at.to_string(),
    })
    .to_string()
}

fn create_token_response() -> String {
    format!(
        "{{\"{}\":\"{}\",\"{}\":3600}}",
        credentials_constants::ACCESS_TOKEN,
        "new-access-token",
        credentials_constants::EXPIRES_IN
    )
}

fn assert_connector_error(
    e: &FederationError,
    code: FederationSourceErrorCode,
    message: Option<&str>,
) {
    let connector = e
        .as_connector_exception()
        .expect("Expected AthenaConnectorException");
    assert_eq!(code, connector.code);
    match message {
        Some(m) => assert_eq!(Some(m), connector.details_message.as_deref()),
        None => assert!(connector.details_message.as_deref().is_some()),
    }
}

#[tokio::test]
async fn test_get_credential_map_when_valid_unexpired_token() {
    let mut s = set_up();
    s.mock_secret_response(&create_oauth_secret_json_with_token(
        TEST_ACCESS_TOKEN,
        now(),
    ));
    let credential_map = s.credentials_provider.get_credential_map().await.unwrap();
    assert_eq!(
        Some(&TEST_ACCESS_TOKEN.to_string()),
        credential_map.get(credentials_constants::ACCESS_TOKEN_PROPERTY)
    );
}

#[tokio::test]
async fn test_get_credential_map_when_expired_token() {
    let mut s = set_up();
    s.mock_secret_response(&create_oauth_secret_json_with_token(
        "expired-token",
        now() - 4000,
    ));
    s.mock_http_client_for_token_fetch(&create_token_response());
    let credential_map = s.credentials_provider.get_credential_map().await.unwrap();
    assert_eq!(
        Some(&"new-access-token".to_string()),
        credential_map.get(credentials_constants::ACCESS_TOKEN_PROPERTY)
    );
}

#[tokio::test]
async fn test_get_credential_map_when_required_fields_missing_throws_exception() {
    let mut s = set_up();
    s.mock_secret_response(&create_oauth_secret_json());
    s.mock_http_client_for_token_fetch("{\"some_field\":\"value\"}");
    let e = s
        .credentials_provider
        .get_credential_map()
        .await
        .unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InvalidResponseException,
        Some("Response missing access_token or expires_in fields"),
    );
}

#[tokio::test]
async fn test_get_credential_map_when_io_exception_throws_exception() {
    let mut s = set_up();
    s.mock_secret_response(&create_oauth_secret_json());
    let error_message = "Network error";
    s.mock_http_error(HttpClientError::Io(error_message.into()));
    let e = s
        .credentials_provider
        .get_credential_map()
        .await
        .unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::OperationTimeoutException,
        Some(error_message),
    );
}

#[tokio::test]
async fn test_get_credential_map_when_interrupted_exception_throws_exception() {
    let mut s = set_up();
    s.mock_secret_response(&create_oauth_secret_json());
    let error_message = "Operation interrupted";
    s.mock_http_error(HttpClientError::Interrupted(error_message.into()));
    let e = s
        .credentials_provider
        .get_credential_map()
        .await
        .unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::OperationTimeoutException,
        Some(error_message),
    );
}

#[tokio::test]
async fn test_get_credential_when_secret_not_found_throws_exception() {
    let s = set_up();
    s.mock_http_response(200, "{\"access_token\":\"abc\",\"expires_in\":3600}");
    s.mock_put_error(AwsServiceError::service(
        AwsService::SecretsManager,
        Some("ResourceNotFoundException".into()),
        Some("secret missing".into()),
    ));
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::EntityNotFoundException,
        Some("secret missing"),
    );
}

#[tokio::test]
async fn test_get_credential_when_json_processing_exception_throws_exception() {
    let s = set_up();
    s.mock_http_response(200, "not-json");
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InvalidResponseException,
        None,
    );
}

#[tokio::test]
async fn test_get_credential_when_http_unauthorized_throws_invalid_credentials() {
    let s = set_up();
    s.mock_http_response(401, "");
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InvalidCredentialsException,
        Some("HTTP Status: 401"),
    );
}

#[tokio::test]
async fn test_get_credential_when_http_rate_limited_throws_throttling_exception() {
    let s = set_up();
    s.mock_http_response(429, "");
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::ThrottlingException,
        Some("HTTP Status: 429"),
    );
}

#[tokio::test]
async fn test_get_credential_when_http_unexpected_status_throws_invalid_response() {
    let s = set_up();
    s.mock_http_response(500, "");
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InvalidResponseException,
        Some("Unexpected HTTP status: 500"),
    );
}

#[tokio::test]
async fn test_get_credential_when_secret_update_fails_with_resource_not_found() {
    let s = set_up();
    s.mock_http_response(200, "{\"access_token\":\"abc\",\"expires_in\":3600}");
    s.mock_put_error(AwsServiceError::service(
        AwsService::SecretsManager,
        Some("ResourceNotFoundException".into()),
        Some("secret not found".into()),
    ));
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::EntityNotFoundException,
        Some("secret not found"),
    );
}

#[tokio::test]
async fn test_get_credential_when_secret_update_fails_with_secrets_manager_exception() {
    let s = set_up();
    s.mock_http_response(200, "{\"access_token\":\"abc\",\"expires_in\":3600}");
    s.mock_put_error(AwsServiceError::service(
        AwsService::SecretsManager,
        None,
        Some("SM failure".into()),
    ));
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InternalServiceException,
        Some("SM failure"),
    );
}

#[tokio::test]
async fn test_get_credential_when_secret_update_fails_puts_java_get_message_in_error_details() {
    let s = set_up();
    s.mock_http_response(200, "{\"access_token\":\"abc\",\"expires_in\":3600}");
    s.mock_put_error(
        AwsServiceError::service(
            AwsService::SecretsManager,
            Some("ThrottlingException".into()),
            Some("Rate exceeded".into()),
        )
        .with_error_details()
        .with_status_code(400)
        .with_request_id("req-1"),
    );
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert_connector_error(
        &e,
        FederationSourceErrorCode::InternalServiceException,
        Some("Rate exceeded (Service: SecretsManager, Status Code: 400, Request ID: req-1)"),
    );
}

#[tokio::test]
async fn test_get_credential_when_secret_update_fails_with_client_error_passes_it_through() {
    let s = set_up();
    s.mock_http_response(200, "{\"access_token\":\"abc\",\"expires_in\":3600}");
    let client_error = AwsServiceError::client("Unable to execute HTTP request: Connect timed out");
    s.mock_put_error(client_error.clone());
    let e = s.credentials_provider.get_credential().await.unwrap_err();
    assert!(
        matches!(&e, FederationError::AwsService(inner) if inner == &client_error),
        "{e:?}"
    );
}
