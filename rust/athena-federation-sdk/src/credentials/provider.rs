use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value as JsonValue;

use crate::credentials::http_client::ReqwestHttpClient;
use crate::credentials::Credentials;
use crate::credentials::CredentialsProvider;
use crate::credentials::DefaultCredentials;
use crate::credentials::{OAuthCredentialsProvider, OAuthFlow};
use crate::credentials::{PASSWORD, USERNAME};
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::security::{AwsRequestOverrideConfiguration, CachableSecretsManager};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultCredentialsProvider {
    default_credentials: DefaultCredentials,
}

impl DefaultCredentialsProvider {
    pub fn new(secret_string: &str) -> Result<DefaultCredentialsProvider> {
        let original_map: HashMap<String, serde_json::Value> = serde_json::from_str(secret_string)
            .map_err(|e| {
                FederationError::connector_with_details(
                    FederationSourceErrorCode::InternalServiceException,
                    "Could not deserialize RDS credentials into HashMap: ",
                    e.to_string(),
                )
            })?;
        let mut rds_secrets: HashMap<String, Option<String>> = HashMap::new();
        for (key, value) in original_map {
            if key.eq_ignore_ascii_case(USERNAME) || key.eq_ignore_ascii_case(PASSWORD) {
                let value = match value {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) => Some(s),
                    other => {
                        return Err(FederationError::class_cast(
                            json_type_name(&other),
                            "java.lang.String",
                        ))
                    }
                };
                rds_secrets.insert(key.to_lowercase(), value);
            }
        }
        let user = rds_secrets.get(USERNAME).cloned().flatten();
        let password = rds_secrets.get(PASSWORD).cloned().flatten();
        Ok(DefaultCredentialsProvider {
            default_credentials: DefaultCredentials::new_nullable(
                user.as_deref(),
                password.as_deref(),
            )?,
        })
    }

    pub fn credential(&self) -> &DefaultCredentials {
        &self.default_credentials
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Bool(_) => "java.lang.Boolean",
        serde_json::Value::Number(n) if n.is_f64() => "java.lang.Double",
        serde_json::Value::Number(n) if n.as_i64().is_some_and(|v| i32::try_from(v).is_ok()) => {
            "java.lang.Integer"
        }
        serde_json::Value::Number(n) if n.is_i64() => "java.lang.Long",
        serde_json::Value::Number(_) => "java.math.BigInteger",
        serde_json::Value::Array(_) => "java.util.ArrayList",
        serde_json::Value::Object(_) => "java.util.LinkedHashMap",
        _ => "java.lang.Object",
    }
}

#[async_trait]
impl CredentialsProvider for DefaultCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        Ok(self.default_credentials.clone().into())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticCredentialsProvider {
    default_credentials: DefaultCredentials,
}

impl StaticCredentialsProvider {
    pub fn new(default_credentials: DefaultCredentials) -> Result<StaticCredentialsProvider> {
        if default_credentials.user().trim().is_empty()
            || default_credentials.password().trim().is_empty()
        {
            return Err(FederationError::invalid_input(
                "User or password must not be blank.",
            ));
        }
        Ok(StaticCredentialsProvider {
            default_credentials,
        })
    }

    pub fn credential(&self) -> &DefaultCredentials {
        &self.default_credentials
    }
}

#[async_trait]
impl CredentialsProvider for StaticCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        Ok(self.default_credentials.clone().into())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn to_string_map(map: serde_json::Map<String, JsonValue>) -> HashMap<String, String> {
    map.into_iter()
        .filter_map(|(k, v)| match v {
            JsonValue::Null => None,
            JsonValue::String(s) => Some((k, s)),
            other => Some((k, other.to_string())),
        })
        .collect()
}

pub async fn create_credential_provider(
    secret_name: Option<&str>,
    secrets_manager: &Arc<CachableSecretsManager>,
    oauth_flow: Option<Box<dyn OAuthFlow>>,
    request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
) -> Result<Option<Box<dyn CredentialsProvider>>> {
    let Some(secret_name) = secret_name.filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let secret_string = secrets_manager
        .get_secret(secret_name, request_override_configuration)
        .await?;
    let secret_map = serde_json::from_str::<serde_json::Map<String, JsonValue>>(&secret_string)
        .map(to_string_map)
        .map_err(|e| {
            FederationError::connector_with_details(
                FederationSourceErrorCode::InternalServiceException,
                "Could not deserialize credentials into HashMap: ",
                e.to_string(),
            )
        })?;

    if let Some(flow) = oauth_flow {
        match flow.is_oauth_configured(&secret_map) {
            Ok(true) => {
                return Ok(Some(Box::new(OAuthCredentialsProvider::new(
                    flow,
                    secret_name,
                    secret_map,
                    Arc::clone(secrets_manager),
                    Arc::new(ReqwestHttpClient::new()),
                ))))
            }
            Ok(false) => {}
            Err(e) => {
                let message = e.to_string();
                return Err(FederationError::connector_with_details(
                    FederationSourceErrorCode::InternalServiceException,
                    format!("Failed to create OAuth provider: {message}"),
                    message,
                ));
            }
        }
    }
    Ok(Some(Box::new(DefaultCredentialsProvider::new(
        &secret_string,
    )?)))
}
