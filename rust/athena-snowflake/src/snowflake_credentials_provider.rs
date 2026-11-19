use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use athena_federation_sdk::credentials::{
    Credentials, CredentialsProvider, DefaultCredentials, HttpClient, HttpRequest,
    ReqwestHttpClient,
};
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_federation_sdk::security::{
    AwsRequestOverrideConfiguration, CachableSecretsManager, SecretsManagerClient,
};
use athena_federation_sdk::{FederationError, Result};
use base64::Engine;
use serde_json::{Map, Value};

use crate::snowflake_constants::{self, is_blank};
use crate::utils::{SnowflakeAuthType, SnowflakeAuthUtils};

pub struct SnowflakeCredentialsProvider {
    oauth_secret_name: String,
    secrets_manager: CachableSecretsManager,
    request_override_configuration: Option<AwsRequestOverrideConfiguration>,
    http_client: Arc<dyn HttpClient>,
}

impl SnowflakeCredentialsProvider {
    pub const ACCESS_TOKEN: &'static str = "access_token";
    pub const FETCHED_AT: &'static str = "fetched_at";
    pub const REFRESH_TOKEN: &'static str = "refresh_token";
    pub const EXPIRES_IN: &'static str = "expires_in";

    pub fn new(
        oauth_secret_name: impl Into<String>,
        secrets_client: Arc<dyn SecretsManagerClient>,
        request_override_configuration: Option<AwsRequestOverrideConfiguration>,
    ) -> SnowflakeCredentialsProvider {
        Self::new_with_http_client(
            oauth_secret_name,
            secrets_client,
            request_override_configuration,
            Arc::new(ReqwestHttpClient::new()),
        )
    }

    pub fn new_with_http_client(
        oauth_secret_name: impl Into<String>,
        secrets_client: Arc<dyn SecretsManagerClient>,
        request_override_configuration: Option<AwsRequestOverrideConfiguration>,
        http_client: Arc<dyn HttpClient>,
    ) -> SnowflakeCredentialsProvider {
        SnowflakeCredentialsProvider {
            oauth_secret_name: oauth_secret_name.into(),
            secrets_manager: CachableSecretsManager::new(secrets_client),
            request_override_configuration,
            http_client,
        }
    }

    pub fn oauth_secret_name(&self) -> &str {
        &self.oauth_secret_name
    }

    pub fn request_override_configuration(&self) -> Option<&AwsRequestOverrideConfiguration> {
        self.request_override_configuration.as_ref()
    }

    async fn resolve_credential_map(&self) -> Result<HashMap<String, String>> {
        let secret_string = self
            .secrets_manager
            .get_secret(
                &self.oauth_secret_name,
                self.request_override_configuration.as_ref(),
            )
            .await
            .map_err(|e| match e {
                FederationError::NullPointer(_) => {
                    FederationError::illegal_argument("argument \"content\" is null")
                }
                other => other,
            })?;
        let mut secret_json: Map<String, Value> =
            serde_json::from_str(&secret_string).map_err(runtime_error)?;
        let secret_map = string_map(&secret_json);
        let auth_type = SnowflakeAuthUtils::determine_auth_type(&secret_map)?;
        SnowflakeAuthUtils::validate_credentials(&secret_map, auth_type)?;
        match auth_type {
            SnowflakeAuthType::SnowflakeJwt => handle_key_pair_authentication(&secret_map),
            SnowflakeAuthType::Oauth => {
                self.handle_oauth_authentication(&secret_map, &mut secret_json)
                    .await
            }
            SnowflakeAuthType::Snowflake => handle_password_authentication(&secret_map),
        }
    }

    async fn handle_oauth_authentication(
        &self,
        oauth_config: &HashMap<String, String>,
        secret_json: &mut Map<String, Value>,
    ) -> Result<HashMap<String, String>> {
        let access_token = self
            .fetch_access_token_from_secret(oauth_config, secret_json)
            .await?;
        let username = SnowflakeAuthUtils::username(oauth_config)?;
        tracing::debug!("Using OAuth authentication for user: {username}");
        Ok(HashMap::from([
            (snowflake_constants::USER.to_string(), username),
            (snowflake_constants::PASSWORD.to_string(), access_token),
            (
                snowflake_constants::AUTHENTICATOR.to_string(),
                SnowflakeAuthType::Oauth.as_str().to_string(),
            ),
        ]))
    }

    async fn fetch_access_token_from_secret(
        &self,
        oauth_config: &HashMap<String, String>,
        secret_json: &mut Map<String, Value>,
    ) -> Result<String> {
        let client_id = require_non_blank(oauth_config, snowflake_constants::CLIENT_ID)?;
        let token_endpoint = require_non_blank(oauth_config, snowflake_constants::TOKEN_URL)?;
        let redirect_uri = require_non_blank(oauth_config, snowflake_constants::REDIRECT_URI)?;
        let client_secret = require_non_blank(oauth_config, snowflake_constants::CLIENT_SECRET)?;
        let auth_code = require_non_blank(oauth_config, snowflake_constants::AUTH_CODE)?;
        let Some(access_token) = oauth_config.get(Self::ACCESS_TOKEN) else {
            tracing::debug!("First time auth. Using authorization_code...");
            let body = format!(
                "grant_type=authorization_code&code={auth_code}&redirect_uri={redirect_uri}"
            );
            let token_json = self
                .request_token(&body, token_endpoint, client_id, client_secret)
                .await?;
            return self
                .save_token_to_secrets_manager(token_json, secret_json)
                .await;
        };
        let expires_in = parse_long(oauth_config.get(Self::EXPIRES_IN).map(String::as_str))?;
        let fetched_at = parse_long(Some(
            oauth_config
                .get(Self::FETCHED_AT)
                .map_or("0", String::as_str),
        ))?;
        let now = now_seconds();
        if now.wrapping_sub(fetched_at) < expires_in.wrapping_sub(60) {
            tracing::debug!("Access token still valid.");
            return Ok(access_token.clone());
        }
        tracing::debug!("Access token expired. Using refresh_token...");
        let refresh_token = oauth_config
            .get(Self::REFRESH_TOKEN)
            .ok_or_else(|| FederationError::NullPointer(None))?;
        let body = format!(
            "grant_type=refresh_token&refresh_token={}",
            java_url_encode(refresh_token)
        );
        let mut refreshed = self
            .request_token(&body, token_endpoint, client_id, client_secret)
            .await?;
        refreshed.insert(
            Self::REFRESH_TOKEN.to_string(),
            Value::String(refresh_token.clone()),
        );
        self.save_token_to_secrets_manager(refreshed, secret_json)
            .await
    }

    async fn request_token(
        &self,
        request_body: &str,
        token_endpoint: &str,
        client_id: &str,
        client_secret: &str,
    ) -> Result<Map<String, Value>> {
        let auth_header = base64::engine::general_purpose::STANDARD
            .encode(format!("{client_id}:{client_secret}"));
        let request = HttpRequest::builder(token_endpoint)
            .header("Authorization", format!("Basic {auth_header}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .post(request_body)
            .build();
        let response = self
            .http_client
            .send(request)
            .await
            .map_err(runtime_error)?;
        let response_code = response.status_code();
        let success = (200..300).contains(&response_code);
        if !success && response.body().is_empty() {
            return Err(FederationError::NullPointer(None));
        }
        let body: String = response.body().lines().collect();
        if response_code != 200 {
            tracing::error!("OAuth token request failed with status: {response_code} - {body}");
            return Err(FederationError::connector(
                FederationSourceErrorCode::InvalidResponseException,
                format!("OAuth authentication failed with status: {response_code}"),
            ));
        }
        let mut token_json: Map<String, Value> =
            serde_json::from_str(&body).map_err(runtime_error)?;
        token_json.insert(Self::FETCHED_AT.to_string(), Value::from(now_seconds()));
        Ok(token_json)
    }

    async fn save_token_to_secrets_manager(
        &self,
        mut token_json: Map<String, Value>,
        secret_json: &mut Map<String, Value>,
    ) -> Result<String> {
        let fetched_at = now_seconds();
        token_json.insert(Self::FETCHED_AT.to_string(), Value::from(fetched_at));
        let access_token = node_text(&token_json, Self::ACCESS_TOKEN)?;
        let refresh_token = node_text(&token_json, Self::REFRESH_TOKEN)?;
        let expires_in = node_int(&token_json, Self::EXPIRES_IN)?;
        secret_json.insert(
            Self::ACCESS_TOKEN.to_string(),
            Value::String(access_token.clone()),
        );
        secret_json.insert(
            Self::REFRESH_TOKEN.to_string(),
            Value::String(refresh_token),
        );
        secret_json.insert(
            Self::EXPIRES_IN.to_string(),
            Value::String(expires_in.to_string()),
        );
        secret_json.insert(
            Self::FETCHED_AT.to_string(),
            Value::String(fetched_at.to_string()),
        );
        let save = async {
            let updated_secret_string =
                serde_json::to_string(&*secret_json).map_err(runtime_error)?;
            self.secrets_manager
                .secrets_manager()
                .put_secret_value(
                    &self.oauth_secret_name,
                    &updated_secret_string,
                    self.request_override_configuration.as_ref(),
                )
                .await
                .map_err(FederationError::from)
        };
        if let Err(e) = save.await {
            tracing::error!("Failed to save updated secret: {e}");
            return Err(FederationError::runtime_with_cause(
                "Failed to save updated secret: ",
                e,
            ));
        }
        Ok(access_token)
    }
}

fn runtime_error(error: impl std::error::Error + Send + Sync + 'static) -> FederationError {
    FederationError::other(error)
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

fn string_map(secret_json: &Map<String, Value>) -> HashMap<String, String> {
    secret_json
        .iter()
        .filter_map(|(key, value)| match value {
            Value::Null => None,
            Value::String(text) => Some((key.clone(), text.clone())),
            other => Some((key.clone(), other.to_string())),
        })
        .collect()
}

fn require_non_blank<'a>(config: &'a HashMap<String, String>, key: &str) -> Result<&'a str> {
    let value = config.get(key).map(String::as_str);
    match value {
        Some(text) if !is_blank(value) => Ok(text),
        _ => Err(FederationError::illegal_argument(format!(
            "Missing required property: {key}"
        ))),
    }
}

fn parse_long(value: Option<&str>) -> Result<i64> {
    let text = value.ok_or_else(|| {
        FederationError::NumberFormat("Cannot parse null string: null".to_string())
    })?;
    text.parse::<i64>()
        .map_err(|_| FederationError::NumberFormat(format!("For input string: \"{text}\"")))
}

fn missing_node(key: &str) -> FederationError {
    FederationError::null_pointer(format!(
        "Cannot invoke \"com.fasterxml.jackson.databind.JsonNode.asText()\" because the return value of \"com.fasterxml.jackson.databind.node.ObjectNode.get(String)\" is null ({key})"
    ))
}

fn node_text(node: &Map<String, Value>, key: &str) -> Result<String> {
    match node.get(key) {
        None => Err(missing_node(key)),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(Value::Null) => Ok("null".to_string()),
        Some(Value::Object(_) | Value::Array(_)) => Ok(String::new()),
        Some(other) => Ok(other.to_string()),
    }
}

fn node_int(node: &Map<String, Value>, key: &str) -> Result<i64> {
    match node.get(key) {
        None => Err(missing_node(key)),
        Some(Value::Number(number)) => Ok(number
            .as_i64()
            .or_else(|| number.as_f64().map(truncate_to_int))
            .map_or(0, |n| i64::from(int_of(n)))),
        Some(Value::String(text)) => Ok(java_text_as_int(text)),
        Some(Value::Bool(true)) => Ok(1),
        Some(_) => Ok(0),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Jackson asInt truncates doubles toward zero"
)]
fn truncate_to_int(value: f64) -> i64 {
    value as i64
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "Jackson asInt keeps the low 32 bits of a long"
)]
fn int_of(value: i64) -> i32 {
    value as i32
}

fn java_text_as_int(text: &str) -> i64 {
    let trimmed = text.trim();
    trimmed
        .parse::<i32>()
        .map(i64::from)
        .or_else(|_| {
            trimmed
                .parse::<f64>()
                .map(|d| i64::from(int_of(truncate_to_int(d))))
        })
        .unwrap_or(0)
}

fn java_url_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push('+'),
            other => {
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                encoded.push('%');
                encoded.push(char::from(HEX[usize::from(other >> 4)]));
                encoded.push(char::from(HEX[usize::from(other & 0x0F)]));
            }
        }
    }
    encoded
}

fn handle_key_pair_authentication(
    oauth_config: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let username = SnowflakeAuthUtils::username(oauth_config)?;
    let mut credential_map =
        HashMap::from([(snowflake_constants::USER.to_string(), username.clone())]);
    for key in [
        snowflake_constants::PEM_PRIVATE_KEY,
        snowflake_constants::PEM_PRIVATE_KEY_PASSPHRASE,
    ] {
        if let Some(value) = oauth_config.get(key) {
            credential_map.insert(key.to_string(), value.clone());
        }
    }
    tracing::debug!("Using key-pair authentication for user: {username}");
    Ok(credential_map)
}

fn handle_password_authentication(
    oauth_config: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    let username = SnowflakeAuthUtils::username(oauth_config)?;
    let password = SnowflakeAuthUtils::password(oauth_config)?;
    tracing::debug!("Using password authentication for user: {username}");
    Ok(HashMap::from([
        (snowflake_constants::USER.to_string(), username),
        (snowflake_constants::PASSWORD.to_string(), password),
    ]))
}

fn passes_through(error: &FederationError) -> bool {
    matches!(
        error,
        FederationError::IllegalArgument(_)
            | FederationError::NumberFormat(_)
            | FederationError::AthenaConnector(_)
    )
}

#[async_trait]
impl CredentialsProvider for SnowflakeCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        let credential_map = self.get_credential_map().await?;
        let credentials = DefaultCredentials::new_nullable(
            credential_map
                .get(snowflake_constants::USER)
                .map(String::as_str),
            credential_map
                .get(snowflake_constants::PASSWORD)
                .map(String::as_str),
        )?;
        Ok(credentials.into())
    }

    async fn get_credential_map(&self) -> Result<HashMap<String, String>> {
        self.resolve_credential_map().await.map_err(|e| {
            if passes_through(&e) {
                e
            } else {
                let message = format!("Error retrieving Snowflake credentials: {e}");
                FederationError::runtime_with_cause(message, e)
            }
        })
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use athena_federation_sdk::exceptions::JavaThrowable;

    use super::{runtime_error, FederationError};

    #[test]
    fn json_errors_keep_their_source_and_message() {
        let json_error = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
        let expected = json_error.to_string();
        let error = runtime_error(json_error);
        assert_eq!(expected, error.to_string());
        assert_eq!("java.lang.RuntimeException", error.java_class_name());
        assert!(error.source().is_some());
        assert!(matches!(
            &error,
            FederationError::Other(source) if source.downcast_ref::<serde_json::Error>().is_some()
        ));
    }
}
