use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use super::credentials::Credentials;
use super::credentials_constants;
use super::credentials_provider::CredentialsProvider;
use super::http_client::{HttpClient, HttpClientError, HttpRequest};
use super::oauth_access_token_credentials::OAuthAccessTokenCredentials;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::java_cast::double_to_long;
use crate::security::{AwsService, AwsServiceError, CachableSecretsManager};

const RESOURCE_NOT_FOUND_EXCEPTION: &str = "ResourceNotFoundException";

pub trait OAuthFlow: Send + Sync {
    fn is_oauth_configured(&self, secret_map: &HashMap<String, String>) -> Result<bool>;

    fn build_token_request(&self, secret_map: &HashMap<String, String>) -> Result<HttpRequest>;
}

pub struct OAuthCredentialsProvider {
    flow: Box<dyn OAuthFlow>,
    secret_name: String,
    secrets_manager: Arc<CachableSecretsManager>,
    secret_map: Mutex<HashMap<String, String>>,
    http_client: Arc<dyn HttpClient>,
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn parse_long(value: &str) -> Result<i64> {
    value
        .parse::<i64>()
        .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{value}\"")))
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
            .or_else(|| n.as_f64().map(double_to_long))
            .unwrap_or(0),
        JsonValue::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        JsonValue::Bool(b) => i64::from(*b),
        _ => 0,
    }
}

fn connector_error(
    code: FederationSourceErrorCode,
    message: impl Into<String>,
    details: impl Into<String>,
) -> FederationError {
    FederationError::connector_with_details(code, message, details)
}

fn service_error(
    code: FederationSourceErrorCode,
    message: impl Into<String>,
    e: &AwsServiceError,
) -> FederationError {
    match e.get_message() {
        Some(details) => connector_error(code, message, details),
        None => FederationError::connector(code, message),
    }
}

impl OAuthCredentialsProvider {
    pub fn new(
        flow: Box<dyn OAuthFlow>,
        secret_name: &str,
        secret_map: HashMap<String, String>,
        secrets_manager: Arc<CachableSecretsManager>,
        http_client: Arc<dyn HttpClient>,
    ) -> OAuthCredentialsProvider {
        OAuthCredentialsProvider {
            flow,
            secret_name: secret_name.to_string(),
            secrets_manager,
            secret_map: Mutex::new(secret_map),
            http_client,
        }
    }

    fn lock_secret_map(&self) -> MutexGuard<'_, HashMap<String, String>> {
        self.secret_map
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn cached_access_token(snapshot: &HashMap<String, String>) -> Result<Option<String>> {
        if let Some(cached) = snapshot.get(credentials_constants::ACCESS_TOKEN) {
            let expires_in = parse_long(
                snapshot
                    .get(credentials_constants::EXPIRES_IN)
                    .map_or("0", String::as_str),
            )?;
            let fetched_at = parse_long(
                snapshot
                    .get(credentials_constants::FETCHED_AT)
                    .map_or("0", String::as_str),
            )?;
            if now_seconds() - fetched_at < expires_in - 60 {
                tracing::debug!("Access token still valid");
                return Ok(Some(cached.clone()));
            }
            tracing::debug!("Access token expired, fetching new token");
        }
        Ok(None)
    }

    async fn fetch_token_response(&self, snapshot: &HashMap<String, String>) -> Result<JsonValue> {
        let request = self.flow.build_token_request(snapshot)?;
        let response = self.http_client.send(request).await.map_err(|e| match e {
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
                format!("HTTP Status: {sc}"),
            ));
        }
        if sc == 429 {
            return Err(connector_error(
                FederationSourceErrorCode::ThrottlingException,
                "Failed to fetch access token: Rate limit exceeded",
                format!("HTTP Status: {sc}"),
            ));
        }
        if sc != 200 {
            return Err(connector_error(
                FederationSourceErrorCode::InvalidResponseException,
                format!("Failed to fetch access token: HTTP {sc}"),
                format!("Unexpected HTTP status: {sc}"),
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
        Ok(token_response)
    }

    async fn store_access_token(
        &self,
        access_token: &str,
        expires_in: i64,
        fetched_at: i64,
    ) -> Result<()> {
        let secret_string = {
            let mut secret_map = self.lock_secret_map();
            secret_map.insert(
                credentials_constants::ACCESS_TOKEN.to_string(),
                access_token.to_string(),
            );
            secret_map.insert(
                credentials_constants::EXPIRES_IN.to_string(),
                expires_in.to_string(),
            );
            secret_map.insert(
                credentials_constants::FETCHED_AT.to_string(),
                fetched_at.to_string(),
            );
            serde_json::to_string(&*secret_map).map_err(|e| {
                connector_error(
                    FederationSourceErrorCode::InvalidResponseException,
                    "Failed to serialize OAuth credentials",
                    e.to_string(),
                )
            })?
        };

        let secret_name = self.secret_name.as_str();
        self.secrets_manager
            .secrets_manager()
            .put_secret_value(secret_name, &secret_string)
            .await
            .map_err(|e| {
                if !e.is_service_exception_of(AwsService::SecretsManager) {
                    FederationError::from(e)
                } else if e.is_error_code(RESOURCE_NOT_FOUND_EXCEPTION) {
                    service_error(
                        FederationSourceErrorCode::EntityNotFoundException,
                        format!("Secret not found: {secret_name}"),
                        &e,
                    )
                } else {
                    service_error(
                        FederationSourceErrorCode::InternalServiceException,
                        "Failed to update OAuth credentials in Secrets Manager",
                        &e,
                    )
                }
            })?;
        Ok(())
    }

    async fn get_oauth_access_token(&self) -> Result<String> {
        let snapshot = self.lock_secret_map().clone();
        if let Some(cached) = Self::cached_access_token(&snapshot)? {
            return Ok(cached);
        }
        let token_response = self.fetch_token_response(&snapshot).await?;
        let (Some(access_token_node), Some(expires_in_node)) = (
            token_response.get(credentials_constants::ACCESS_TOKEN),
            token_response.get(credentials_constants::EXPIRES_IN),
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

        self.store_access_token(&access_token, expires_in, fetched_at)
            .await?;
        Ok(access_token)
    }
}

#[async_trait]
impl CredentialsProvider for OAuthCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        let access_token = self.get_oauth_access_token().await?;
        Ok(OAuthAccessTokenCredentials::new(&access_token)?.into())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
