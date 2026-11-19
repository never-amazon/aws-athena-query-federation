use std::collections::HashMap;
use std::sync::Arc;

use aws_sdk_glue::types::{AuthenticationConfiguration, Connection};

use super::environment_constants;
use super::glue_connection_client::{default_glue_client, GlueConnectionClient};
use crate::exceptions::{FederationError, Result};

const GLUE_ENDPOINT: &str = "GLUE_ENDPOINT";

fn java_split(value: &str, separator: char) -> Vec<&str> {
    let mut parts: Vec<&str> = value.split(separator).collect();
    if parts.len() > 1 {
        while parts.last() == Some(&"") {
            parts.pop();
        }
    }
    parts
}

#[derive(Clone, Default)]
pub struct EnvironmentProperties {
    env_map: Option<HashMap<String, String>>,
    glue_client: Option<Arc<dyn GlueConnectionClient>>,
}

impl EnvironmentProperties {
    pub fn new() -> EnvironmentProperties {
        EnvironmentProperties::default()
    }

    #[must_use]
    pub fn with_env_map(mut self, env_map: HashMap<String, String>) -> EnvironmentProperties {
        self.env_map = Some(env_map);
        self
    }

    #[must_use]
    pub fn with_glue_client(
        mut self,
        glue_client: Arc<dyn GlueConnectionClient>,
    ) -> EnvironmentProperties {
        self.glue_client = Some(glue_client);
        self
    }

    pub async fn create_environment(&self) -> Result<HashMap<String, String>> {
        self.create_environment_with(|properties| {
            self.connection_properties_to_environment(properties)
        })
        .await
    }

    pub async fn create_environment_with<F>(
        &self,
        connection_properties_to_environment: F,
    ) -> Result<HashMap<String, String>>
    where
        F: Fn(HashMap<String, String>) -> HashMap<String, String> + Send + Sync,
    {
        let lambda_environment = self.get_env_map();
        let mut connection_environment = HashMap::new();
        if let Some(glue_connection_name) = lambda_environment
            .get(environment_constants::DEFAULT_GLUE_CONNECTION)
            .filter(|n| !n.trim().is_empty())
        {
            connection_environment.insert(
                environment_constants::DEFAULT_GLUE_CONNECTION.to_string(),
                glue_connection_name.clone(),
            );
            let connection = self.get_glue_connection(glue_connection_name).await?;
            let mut connection_properties_with_secret: HashMap<String, String> = connection
                .connection_properties()
                .map(|props| {
                    props
                        .iter()
                        .map(|(k, v)| (k.as_str().to_string(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            connection_properties_with_secret.extend(Self::authentication_configuration_to_map(
                connection.authentication_configuration(),
            ));
            connection_environment.extend(connection_properties_to_environment(
                connection_properties_with_secret,
            ));
            let athena_properties = connection.athena_properties().cloned().unwrap_or_default();
            connection_environment
                .extend(self.athena_properties_to_environment(&athena_properties));
        }
        connection_environment.extend(lambda_environment);
        Ok(connection_environment)
    }

    pub async fn get_glue_connection(&self, glue_connection_name: &str) -> Result<Connection> {
        let client: Arc<dyn GlueConnectionClient> = if let Some(client) = &self.glue_client {
            client.clone()
        } else {
            let lambda_environment = self.get_env_map();
            Arc::new(
                default_glue_client(lambda_environment.get(GLUE_ENDPOINT).map(String::as_str))
                    .await,
            )
        };
        match client.get_connection(glue_connection_name).await {
            Ok(connection) => {
                tracing::debug!("Successfully retrieved connection {}", glue_connection_name);
                Ok(connection)
            }
            Err(err) => {
                tracing::error!(
                    "Failed to retrieve connection: {}, and parse the connection properties!",
                    glue_connection_name
                );
                Err(FederationError::runtime_from(err.to_string(), err))
            }
        }
    }

    fn authentication_configuration_to_map(
        auth: Option<&AuthenticationConfiguration>,
    ) -> HashMap<String, String> {
        let mut auth_map = HashMap::new();
        if let Some(secret_arn) = auth
            .and_then(|a| a.secret_arn())
            .filter(|s| !s.trim().is_empty())
        {
            let split_arn = java_split(secret_arn, ':');
            let last = split_arn.last().copied().unwrap_or_default();
            let parts = java_split(last, '-');
            let secret_name = parts[..parts.len().saturating_sub(1)].join("-");
            auth_map.insert(environment_constants::SECRET_NAME.to_string(), secret_name);
        }
        auth_map
    }

    pub fn athena_properties_to_environment(
        &self,
        athena_properties: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut modified = athena_properties.clone();
        if let Some(kms_key_id) = modified.remove(environment_constants::SPILL_KMS_KEY_ID) {
            modified.insert(environment_constants::KMS_KEY_ID.to_string(), kms_key_id);
        }
        modified
    }

    pub fn connection_properties_to_environment(
        &self,
        connection_properties: HashMap<String, String>,
    ) -> HashMap<String, String> {
        connection_properties
    }

    pub fn get_env_map(&self) -> HashMap<String, String> {
        match &self.env_map {
            Some(map) => map.clone(),
            None => std::env::vars().collect(),
        }
    }
}
