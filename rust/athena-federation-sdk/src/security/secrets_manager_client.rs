use async_trait::async_trait;

use super::aws_request_override_configuration::{send_with_override, AwsRequestOverrideConfiguration};
use super::aws_service_error::AwsServiceError;

#[async_trait]
pub trait SecretsManagerClient: Send + Sync {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError>;

    async fn put_secret_value(&self, secret_id: &str, secret_string: &str) -> Result<(), AwsServiceError>;
}

#[async_trait]
impl SecretsManagerClient for aws_sdk_secretsmanager::Client {
    async fn get_secret_value(
        &self,
        secret_id: &str,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<String>, AwsServiceError> {
        let builder = self.get_secret_value().secret_id(secret_id);
        let output = send_with_override!(builder, override_configuration, aws_sdk_secretsmanager::config::Builder)
            .map_err(|e| AwsServiceError::from_sdk_error(&e))?;
        Ok(output.secret_string().map(str::to_string))
    }

    async fn put_secret_value(&self, secret_id: &str, secret_string: &str) -> Result<(), AwsServiceError> {
        self.put_secret_value()
            .secret_id(secret_id)
            .secret_string(secret_string)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(&e))?;
        Ok(())
    }
}
