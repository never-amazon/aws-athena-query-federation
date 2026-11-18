use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use super::credentials_provider::CredentialsProvider;
use super::default_credentials_provider::DefaultCredentialsProvider;
use super::http_client::ReqwestHttpClient;
use super::oauth_credentials_provider::{OAuthCredentialsProvider, OAuthFlow};
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::security::{AwsRequestOverrideConfiguration, CachableSecretsManager};

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
