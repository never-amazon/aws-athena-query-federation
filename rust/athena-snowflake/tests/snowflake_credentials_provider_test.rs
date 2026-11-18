use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use athena_federation_sdk::credentials::{
    Credentials, CredentialsProvider, HttpClient, HttpClientError, HttpRequest, HttpResponse,
};
use athena_federation_sdk::lambda::LambdaError;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, SecretsManagerClient,
};
use athena_federation_sdk::FederationError;
use athena_snowflake::SnowflakeCredentialsProvider;
use serde_json::{json, Value};

const TEST_SECRET_NAME: &str = "test-oauth-secret";
const TEST_CLIENT_ID: &str = "test-client-id";
const TEST_CLIENT_SECRET: &str = "test-client-secret";
const TEST_TOKEN_URL: &str = "https://test.snowflakecomputing.com/oauth/token-request";
const TEST_REDIRECT_URI: &str = "https://test.com/callback";
const TEST_AUTH_CODE: &str = "test-auth-code";
const TEST_USERNAME: &str = "test-user";
const TEST_PASSWORD: &str = "test-password";
const TEST_ACCESS_TOKEN: &str = "test-access-token";
const TEST_REFRESH_TOKEN: &str = "test-refresh-token";

type GetCall = (String, Option<AwsRequestOverrideConfiguration>);
type PutCall = (String, String, Option<AwsRequestOverrideConfiguration>);

struct RecordingSecretsManager {
    secret: Option<String>,
    gets: Mutex<Vec<GetCall>>,
    puts: Mutex<Vec<PutCall>>,
}

impl RecordingSecretsManager {
    fn new(secret: Option<&str>) -> Arc<RecordingSecretsManager> {
        Arc::new(RecordingSecretsManager {
            secret: secret.map(ToString::to_string),
            gets: Mutex::new(Vec::new()),
            puts: Mutex::new(Vec::new()),
        })
    }

    fn gets(&self) -> Vec<GetCall> {
        self.gets.lock().unwrap().clone()
    }

    fn puts(&self) -> Vec<PutCall> {
        self.puts.lock().unwrap().clone()
    }
}

#[async_trait]
impl SecretsManagerClient for RecordingSecretsManager {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        self.gets
            .lock()
            .unwrap()
            .push((secret_id.to_string(), override_configuration.cloned()));
        Ok(self.secret.clone())
    }

    async fn put_secret_value(
        &self,
        secret_id: &str,
        secret_string: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<(), AwsServiceError> {
        self.puts.lock().unwrap().push((
            secret_id.to_string(),
            secret_string.to_string(),
            override_configuration.cloned(),
        ));
        Ok(())
    }
}

struct FakeHttpClient {
    response: Result<HttpResponse, HttpClientError>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeHttpClient {
    fn responding(status_code: u16, body: &str) -> Arc<FakeHttpClient> {
        Arc::new(FakeHttpClient {
            response: Ok(HttpResponse::new(status_code, body)),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn failing(error: HttpClientError) -> Arc<FakeHttpClient> {
        Arc::new(FakeHttpClient {
            response: Err(error),
            requests: Mutex::new(Vec::new()),
        })
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpClient for FakeHttpClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpClientError> {
        self.requests.lock().unwrap().push(request);
        self.response.clone()
    }
}

fn provider(
    secrets: &Arc<RecordingSecretsManager>,
    http: &Arc<FakeHttpClient>,
    override_configuration: Option<AwsRequestOverrideConfiguration>,
) -> SnowflakeCredentialsProvider {
    SnowflakeCredentialsProvider::new_with_http_client(
        TEST_SECRET_NAME,
        secrets.clone(),
        override_configuration,
        http.clone(),
    )
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn create_oauth_secret_json() -> String {
    json!({
        "client_id": TEST_CLIENT_ID,
        "client_secret": TEST_CLIENT_SECRET,
        "token_url": TEST_TOKEN_URL,
        "redirect_uri": TEST_REDIRECT_URI,
        "auth_code": TEST_AUTH_CODE,
        "username": TEST_USERNAME,
    })
    .to_string()
}

fn create_oauth_secret_json_with_token(access_token: &str, age_seconds: u64) -> String {
    json!({
        "client_id": TEST_CLIENT_ID,
        "client_secret": TEST_CLIENT_SECRET,
        "token_url": TEST_TOKEN_URL,
        "redirect_uri": TEST_REDIRECT_URI,
        "auth_code": TEST_AUTH_CODE,
        "username": TEST_USERNAME,
        "access_token": access_token,
        "refresh_token": TEST_REFRESH_TOKEN,
        "expires_in": "3600",
        "fetched_at": (now_seconds() - age_seconds).to_string(),
    })
    .to_string()
}

fn create_standard_secret_json() -> String {
    json!({ "username": TEST_USERNAME, "password": TEST_PASSWORD }).to_string()
}

fn create_oauth_secret_json_with_empty_auth_code() -> String {
    json!({
        "client_id": TEST_CLIENT_ID,
        "client_secret": TEST_CLIENT_SECRET,
        "token_url": TEST_TOKEN_URL,
        "redirect_uri": TEST_REDIRECT_URI,
        "auth_code": "",
        "username": TEST_USERNAME,
        "password": TEST_PASSWORD,
    })
    .to_string()
}

fn create_oauth_secret_json_missing_client_id() -> String {
    json!({
        "client_secret": TEST_CLIENT_SECRET,
        "token_url": TEST_TOKEN_URL,
        "redirect_uri": TEST_REDIRECT_URI,
        "auth_code": TEST_AUTH_CODE,
        "username": TEST_USERNAME,
    })
    .to_string()
}

fn create_token_response() -> String {
    json!({
        "access_token": TEST_ACCESS_TOKEN,
        "token_type": "Bearer",
        "expires_in": 3600,
        "refresh_token": TEST_REFRESH_TOKEN,
    })
    .to_string()
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn assert_token_request(request: &HttpRequest, expected_body: &str) {
    assert_eq!("POST", request.method());
    assert_eq!(TEST_TOKEN_URL, request.uri());
    assert_eq!(
        Some("Basic dGVzdC1jbGllbnQtaWQ6dGVzdC1jbGllbnQtc2VjcmV0"),
        header(request, "Authorization")
    );
    assert_eq!(
        Some("application/x-www-form-urlencoded"),
        header(request, "Content-Type")
    );
    assert_eq!(Some(expected_body), request.body());
}

fn assert_oauth_map(credential_map: &HashMap<String, String>) {
    assert_eq!(
        Some(TEST_USERNAME),
        credential_map.get("user").map(String::as_str)
    );
    assert_eq!(
        Some(TEST_ACCESS_TOKEN),
        credential_map.get("password").map(String::as_str)
    );
    assert_eq!(
        Some("oauth"),
        credential_map.get("authenticator").map(String::as_str)
    );
}

fn saved_secret(secrets: &RecordingSecretsManager) -> Value {
    let puts = secrets.puts();
    assert_eq!(1, puts.len());
    assert_eq!(TEST_SECRET_NAME, puts[0].0);
    serde_json::from_str(&puts[0].1).unwrap()
}

const AUTH_CODE_BODY: &str =
    "grant_type=authorization_code&code=test-auth-code&redirect_uri=https://test.com/callback";

#[tokio::test]
async fn test_get_credential_with_o_auth_flow() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, None);

    let credentials = provider.get_credential().await.unwrap();
    let Credentials::UserPassword(credentials) = credentials else {
        panic!("expected user and password credentials");
    };

    assert_eq!(TEST_USERNAME, credentials.user());
    assert_eq!(TEST_ACCESS_TOKEN, credentials.password());
    let requests = http.requests();
    assert_eq!(1, requests.len());
    assert_token_request(&requests[0], AUTH_CODE_BODY);
}

#[tokio::test]
async fn test_get_credential_map_with_o_auth_flow() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_oauth_map(&credential_map);
    let saved = saved_secret(&secrets);
    assert_eq!(json!(TEST_ACCESS_TOKEN), saved["access_token"]);
    assert_eq!(json!(TEST_REFRESH_TOKEN), saved["refresh_token"]);
    assert_eq!(json!("3600"), saved["expires_in"]);
    assert!(saved["fetched_at"].is_string());
}

#[tokio::test]
async fn test_get_credential_map_with_existing_token() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json_with_token(
        TEST_ACCESS_TOKEN,
        1000,
    )));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_oauth_map(&credential_map);
    assert!(http.requests().is_empty());
    assert!(secrets.puts().is_empty());
}

#[tokio::test]
async fn test_get_credential_map_with_expired_token() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json_with_token(
        "expired-token",
        4000,
    )));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_oauth_map(&credential_map);
    let requests = http.requests();
    assert_eq!(1, requests.len());
    assert_token_request(
        &requests[0],
        "grant_type=refresh_token&refresh_token=test-refresh-token",
    );
    assert_eq!(
        json!(TEST_ACCESS_TOKEN),
        saved_secret(&secrets)["access_token"]
    );
}

#[tokio::test]
async fn test_get_credential_map_with_standard_credentials() {
    let secrets = RecordingSecretsManager::new(Some(&create_standard_secret_json()));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_eq!(
        Some(TEST_USERNAME),
        credential_map.get("user").map(String::as_str)
    );
    assert_eq!(
        Some(TEST_PASSWORD),
        credential_map.get("password").map(String::as_str)
    );
    assert_eq!(None, credential_map.get("authenticator"));
}

#[tokio::test]
async fn test_get_credential_map_with_empty_auth_code() {
    let secrets =
        RecordingSecretsManager::new(Some(&create_oauth_secret_json_with_empty_auth_code()));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_eq!(
        Some(TEST_USERNAME),
        credential_map.get("user").map(String::as_str)
    );
    assert_eq!(
        Some(TEST_PASSWORD),
        credential_map.get("password").map(String::as_str)
    );
    assert_eq!(None, credential_map.get("authenticator"));
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn test_get_credential_map_with_missing_required_properties() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json_missing_client_id()));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert!(matches!(error, FederationError::IllegalArgument(_)));
    assert!(error
        .to_string()
        .contains("Missing required property: client_id"));
}

#[tokio::test]
async fn test_get_credential_map_with_http_error() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(400, "");
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert_runtime_with_cause(&error);
    assert!(error
        .to_string()
        .contains("Error retrieving Snowflake credential"));
    assert!(secrets.puts().is_empty());
}

#[tokio::test]
async fn test_get_credential_map_with_invalid_json_response() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(200, "invalid json response");
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert_runtime_with_cause(&error);
    assert!(error
        .to_string()
        .contains("Error retrieving Snowflake credentials"));
}

#[tokio::test]
async fn test_get_credential_map_with_io_exception() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::failing(HttpClientError::Io("Connection failed".to_string()));
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert_runtime_with_cause(&error);
    assert!(error
        .to_string()
        .contains("Error retrieving Snowflake credentials"));
    assert!(error.to_string().contains("Connection failed"));
    let report = LambdaError::new(&error).to_report();
    assert!(
        report.contains("\ncause: java.lang.RuntimeException: Connection failed\n"),
        "{report}"
    );
}

fn assert_runtime_with_cause(error: &FederationError) {
    assert_eq!("java.lang.RuntimeException", error.java_class_name());
    assert!(error.source().is_some(), "{error:?}");
}

#[tokio::test]
async fn test_get_credential_map_with_null_secret_string() {
    let secrets = RecordingSecretsManager::new(None);
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert!(matches!(error, FederationError::IllegalArgument(_)));
}

#[tokio::test]
async fn test_get_credential_map_with_invalid_secret_json() {
    let secrets = RecordingSecretsManager::new(Some("invalid json"));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert_runtime_with_cause(&error);
    assert!(error
        .to_string()
        .contains("Error retrieving Snowflake credentials"));
}

#[tokio::test]
async fn test_get_credential_map_with_put_secret_value_exception() {
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_oauth_map(&credential_map);
    assert_eq!(1, secrets.puts().len());
}

#[tokio::test]
async fn test_get_credential_map_passes_request_override_configuration() {
    let override_configuration = AwsRequestOverrideConfiguration::builder()
        .put_header("x-api-call-timeout", "30")
        .build();
    let secrets = RecordingSecretsManager::new(Some(&create_oauth_secret_json()));
    let http = FakeHttpClient::responding(200, &create_token_response());
    let provider = provider(&secrets, &http, Some(override_configuration.clone()));

    provider.get_credential_map().await.unwrap();

    assert_eq!(
        vec![(
            TEST_SECRET_NAME.to_string(),
            Some(override_configuration.clone())
        )],
        secrets.gets()
    );
    let puts = secrets.puts();
    assert_eq!(1, puts.len());
    assert_eq!(Some(override_configuration), puts[0].2);
}

#[tokio::test]
async fn test_get_credential_map_with_uppercase_password() {
    let secret = json!({ "USERNAME": TEST_USERNAME, "PASSWORD": TEST_PASSWORD }).to_string();
    let secrets = RecordingSecretsManager::new(Some(&secret));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_eq!(
        Some(TEST_USERNAME),
        credential_map.get("user").map(String::as_str)
    );
    assert_eq!(
        Some(TEST_PASSWORD),
        credential_map.get("password").map(String::as_str)
    );
}

#[tokio::test]
async fn test_get_credential_map_with_missing_password() {
    let secret = json!({ "username": TEST_USERNAME }).to_string();
    let secrets = RecordingSecretsManager::new(Some(&secret));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let error = provider.get_credential_map().await.unwrap_err();

    assert!(matches!(error, FederationError::IllegalArgument(_)));
    assert_eq!(
        "password is required for password authentication",
        error.to_string()
    );
}

#[tokio::test]
async fn test_get_credential_map_with_null_lowercase_password() {
    let secret = json!({
        "username": TEST_USERNAME,
        "password": null,
        "PASSWORD": TEST_PASSWORD,
    })
    .to_string();
    let secrets = RecordingSecretsManager::new(Some(&secret));
    let http = FakeHttpClient::responding(500, "");
    let provider = provider(&secrets, &http, None);

    let credential_map = provider.get_credential_map().await.unwrap();

    assert_eq!(
        Some(TEST_USERNAME),
        credential_map.get("user").map(String::as_str)
    );
    assert_eq!(
        Some(TEST_PASSWORD),
        credential_map.get("password").map(String::as_str)
    );
}
