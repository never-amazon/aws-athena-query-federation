use std::time::Duration;

use async_trait::async_trait;
use aws_config::timeout::TimeoutConfig;
use aws_config::BehaviorVersion;
use aws_sdk_glue::types::Connection;

use super::environment_constants::EnvironmentConstants;
use crate::security::AwsServiceError;

#[async_trait]
pub trait GlueConnectionClient: Send + Sync {
    async fn get_connection(&self, name: &str) -> Result<Connection, AwsServiceError>;
}

#[async_trait]
impl GlueConnectionClient for aws_sdk_glue::Client {
    async fn get_connection(&self, name: &str) -> Result<Connection, AwsServiceError> {
        let output = self
            .get_connection()
            .name(name)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(&e))?;
        output
            .connection
            .ok_or_else(|| AwsServiceError::new(format!("Glue returned no connection named {}", name)))
    }
}

pub(crate) async fn default_glue_client(endpoint: Option<&str>) -> aws_sdk_glue::Client {
    let timeouts = TimeoutConfig::builder()
        .connect_timeout(Duration::from_millis(EnvironmentConstants::CONNECT_TIMEOUT as u64))
        .build();
    let sdk_config = aws_config::defaults(BehaviorVersion::latest())
        .timeout_config(timeouts)
        .load()
        .await;
    let mut builder = aws_sdk_glue::config::Builder::from(&sdk_config);
    if let Some(endpoint) = endpoint {
        tracing::info!("Using custom endpoint {}", endpoint);
        builder = builder.endpoint_url(endpoint);
    }
    aws_sdk_glue::Client::from_conf(builder.build())
}
