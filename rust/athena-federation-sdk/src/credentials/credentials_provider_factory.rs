use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use super::credentials_provider::CredentialsProvider;
use super::default_credentials_provider::DefaultCredentialsProvider;
use super::initializable_credentials_provider::InitializableCredentialsProvider;
use super::oauth_credentials_provider::OAuthCredentialsProvider;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};
use crate::security::{AwsRequestOverrideConfiguration, CachableSecretsManager};

pub struct CredentialsProviderFactory;

fn to_string_map(map: serde_json::Map<String, JsonValue>) -> HashMap<String, String> {
    map.into_iter()
        .filter_map(|(k, v)| match v {
            JsonValue::Null => None,
            JsonValue::String(s) => Some((k, s)),
            other => Some((k, other.to_string())),
        })
        .collect()
}

impl CredentialsProviderFactory {
    pub async fn create_credential_provider(
        secret_name: Option<&str>,
        secrets_manager: &Arc<CachableSecretsManager>,
        provider: Option<Box<dyn InitializableCredentialsProvider>>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        Self::create_credential_provider_with_request_override_configuration(secret_name, secrets_manager, provider, None)
            .await
    }

    pub async fn create_credential_provider_with_request_override_configuration(
        secret_name: Option<&str>,
        secrets_manager: &Arc<CachableSecretsManager>,
        provider: Option<Box<dyn InitializableCredentialsProvider>>,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        let Some(secret_name) = secret_name.filter(|s| !s.trim().is_empty()) else {
            return Ok(None);
        };
        let secret_string = secrets_manager
            .get_secret_with_override_configuration(secret_name, request_override_configuration)
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

        if let Some(provider) = provider {
            if let Ok(mut oauth_provider) = provider.into_any().downcast::<OAuthCredentialsProvider>() {
                let oauth_result = oauth_provider.is_oauth_configured(&secret_map).and_then(|configured| {
                    if configured {
                        oauth_provider.initialize(secret_name, secret_map.clone(), secrets_manager.clone())?;
                    }
                    Ok(configured)
                });
                match oauth_result {
                    Ok(true) => return Ok(Some(oauth_provider)),
                    Ok(false) => {}
                    Err(e) => {
                        let message = e.to_string();
                        return Err(FederationError::connector_with_details(
                            FederationSourceErrorCode::InternalServiceException,
                            format!("Failed to create OAuth provider: {}", message),
                            message,
                        ));
                    }
                }
            }
        }
        Ok(Some(Box::new(DefaultCredentialsProvider::new(&secret_string)?)))
    }
}
