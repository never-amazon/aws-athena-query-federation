use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use super::credentials::Credentials;
use super::credentials_constants::CredentialsConstants;
use super::credentials_provider::CredentialsProvider;
use super::http_client::{HttpClient, HttpClientError, HttpRequest, ReqwestHttpClient};
use super::initializable_credentials_provider::InitializableCredentialsProvider;
use super::oauth_access_token_credentials::OAuthAccessTokenCredentials;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::security::CachableSecretsManager;

const RESOURCE_NOT_FOUND_EXCEPTION: &str = "ResourceNotFoundException";

pub trait OAuthFlow: Send + Sync {
    fn is_oauth_configured(&self, secret_map: &HashMap<String, String>) -> Result<bool>;

    fn build_token_request(&self, secret_map: &HashMap<String, String>) -> Result<HttpRequest>;
}

pub struct OAuthCredentialsProvider {
    flow: Box<dyn OAuthFlow>,
    secret_name: Option<String>,
    secrets_manager: Option<Arc<CachableSecretsManager>>,
    secret_map: Mutex<HashMap<String, String>>,
    http_client: Option<Arc<dyn HttpClient>>,
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn parse_long(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|_| FederationError::Runtime(format!("For input string: \"{}\"", value)))
}

fn json_as_text(node: &JsonValue) -> String {
    match node {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => String::new(),
        other => other.to_string(),
    }
}

fn json_as_long(node: &JsonValue) -> i64 {
    match node {
        JsonValue::Number(n) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f as i64))
            .unwrap_or(0),
        JsonValue::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        JsonValue::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

fn connector_error(code: FederationSourceErrorCode, message: impl Into<String>, details: impl Into<String>) -> FederationError {
    FederationError::connector_with_details(code, message, details)
}

impl OAuthCredentialsProvider {
    pub fn new(flow: Box<dyn OAuthFlow>) -> OAuthCredentialsProvider {
        OAuthCredentialsProvider {
            flow,
            secret_name: None,
            secrets_manager: None,
            secret_map: Mutex::new(HashMap::new()),
            http_client: None,
        }
    }

    pub fn with_http_client(flow: Box<dyn OAuthFlow>, http_client: Arc<dyn HttpClient>) -> OAuthCredentialsProvider {
        OAuthCredentialsProvider {
            http_client: Some(http_client),
            ..OAuthCredentialsProvider::new(flow)
        }
    }

    pub fn is_oauth_configured(&self, secret_map: &HashMap<String, String>) -> Result<bool> {
        self.flow.is_oauth_configured(secret_map)
    }

    pub fn build_token_request(&self, secret_map: &HashMap<String, String>) -> Result<HttpRequest> {
        self.flow.build_token_request(secret_map)
    }

    fn lock_secret_map(&self) -> MutexGuard<'_, HashMap<String, String>> {
        self.secret_map.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn get_oauth_access_token(&self) -> Result<String> {
        let snapshot = self.lock_secret_map().clone();
        if let Some(cached) = snapshot.get(CredentialsConstants::ACCESS_TOKEN) {
            let expires_in = parse_long(snapshot.get(CredentialsConstants::EXPIRES_IN).map_or("0", String::as_str))?;
            let fetched_at = parse_long(snapshot.get(CredentialsConstants::FETCHED_AT).map_or("0", String::as_str))?;
            if now_seconds() - fetched_at < expires_in - 60 {
                tracing::debug!("Access token still valid");
                return Ok(cached.clone());
            }
            tracing::debug!("Access token expired, fetching new token");
        }

        let http_client = self
            .http_client
            .as_ref()
            .ok_or_else(|| FederationError::NullPointer("httpClient was null".into()))?;
        let request = self.flow.build_token_request(&snapshot)?;
        let response = http_client.send(request).await.map_err(|e| match e {
            HttpClientError::Io(m) => connector_error(
                FederationSourceErrorCode::OperationTimeoutException,
                "Failed to communicate with OAuth endpoint",
                m,
            ),
            HttpClientError::Interrupted(m) => connector_error(
                FederationSourceErrorCode::OperationTimeoutException,
                "OAuth token fetch interrupted",
                m,
            ),
        })?;

        let sc = response.status_code();
        if sc == 401 || sc == 403 {
            return Err(connector_error(
                FederationSourceErrorCode::InvalidCredentialsException,
                "Failed to fetch access token: Invalid credentials",
                format!("HTTP Status: {}", sc),
            ));
        }
        if sc == 429 {
            return Err(connector_error(
                FederationSourceErrorCode::ThrottlingException,
                "Failed to fetch access token: Rate limit exceeded",
                format!("HTTP Status: {}", sc),
            ));
        }
        if sc != 200 {
            return Err(connector_error(
                FederationSourceErrorCode::InvalidResponseException,
                format!("Failed to fetch access token: HTTP {}", sc),
                format!("Unexpected HTTP status: {}", sc),
            ));
        }

        let token_response = if response.body().trim().is_empty() {
            JsonValue::Null
        } else {
            serde_json::from_str::<JsonValue>(response.body()).map_err(|e| {
                connector_error(
                    FederationSourceErrorCode::InvalidResponseException,
                    "Failed to parse OAuth token response",
                    e.to_string(),
                )
            })?
        };
        let (Some(access_token_node), Some(expires_in_node)) = (
            token_response.get(CredentialsConstants::ACCESS_TOKEN),
            token_response.get(CredentialsConstants::EXPIRES_IN),
        ) else {
            return Err(connector_error(
                FederationSourceErrorCode::InvalidResponseException,
                "Invalid OAuth response: Missing required fields",
                "Response missing access_token or expires_in fields",
            ));
        };
        let access_token = json_as_text(access_token_node);
        let expires_in = json_as_long(expires_in_node);
        let fetched_at = now_seconds();

        let secret_string = {
            let mut secret_map = self.lock_secret_map();
            secret_map.insert(CredentialsConstants::ACCESS_TOKEN.to_string(), access_token.clone());
            secret_map.insert(CredentialsConstants::EXPIRES_IN.to_string(), expires_in.to_string());
            secret_map.insert(CredentialsConstants::FETCHED_AT.to_string(), fetched_at.to_string());
            serde_json::to_string(&*secret_map).map_err(|e| {
                connector_error(
                    FederationSourceErrorCode::InvalidResponseException,
                    "Failed to serialize OAuth credentials",
                    e.to_string(),
                )
            })?
        };

        let secret_name = self.secret_name.clone().unwrap_or_else(|| "null".to_string());
        let secrets_manager = self
            .secrets_manager
            .as_ref()
            .ok_or_else(|| FederationError::NullPointer("secretsManager was null".into()))?;
        secrets_manager
            .get_secrets_manager()
            .put_secret_value(&secret_name, &secret_string)
            .await
            .map_err(|e| {
                if e.is_error_code(RESOURCE_NOT_FOUND_EXCEPTION) {
                    connector_error(
                        FederationSourceErrorCode::EntityNotFoundException,
                        format!("Secret not found: {}", secret_name),
                        e.message(),
                    )
                } else if e.error_code().is_some() {
                    connector_error(
                        FederationSourceErrorCode::InternalServiceException,
                        "Failed to update OAuth credentials in Secrets Manager",
                        e.message(),
                    )
                } else {
                    FederationError::from(e)
                }
            })?;
        Ok(access_token)
    }
}

#[async_trait]
impl CredentialsProvider for OAuthCredentialsProvider {
    async fn get_credential(&self) -> Result<Box<dyn Credentials>> {
        let access_token = self.get_oauth_access_token().await?;
        Ok(Box::new(OAuthAccessTokenCredentials::new(&access_token)?))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl InitializableCredentialsProvider for OAuthCredentialsProvider {
    fn initialize(
        &mut self,
        secret_name: &str,
        secret_map: HashMap<String, String>,
        secrets_manager: Arc<CachableSecretsManager>,
    ) -> Result<()> {
        self.secret_name = Some(secret_name.to_string());
        *self.lock_secret_map() = secret_map;
        self.secrets_manager = Some(secrets_manager);
        if self.http_client.is_none() {
            self.http_client = Some(Arc::new(ReqwestHttpClient::new()));
        }
        Ok(())
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }

    fn into_credentials_provider(self: Box<Self>) -> Box<dyn CredentialsProvider> {
        self
    }
}
