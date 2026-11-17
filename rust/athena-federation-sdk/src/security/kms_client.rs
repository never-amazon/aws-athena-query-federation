use async_trait::async_trait;
use aws_sdk_kms::types::DataKeySpec;

use super::aws_request_override_configuration::{send_with_override, AwsRequestOverrideConfiguration};
use super::aws_service_error::{AwsService, AwsServiceError};

#[async_trait]
pub trait KmsClient: Send + Sync {
    async fn generate_data_key(
        &self,
        key_id: &str,
        key_spec: DataKeySpec,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError>;

    async fn generate_random(&self, number_of_bytes: i32) -> Result<Vec<u8>, AwsServiceError>;
}

#[async_trait]
impl KmsClient for aws_sdk_kms::Client {
    async fn generate_data_key(
        &self,
        key_id: &str,
        key_spec: DataKeySpec,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError> {
        let builder = self.generate_data_key().key_id(key_id).key_spec(key_spec);
        let output = send_with_override!(builder, override_configuration, aws_sdk_kms::config::Builder)
            .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Kms, &e))?;
        Ok(output.plaintext().map(|b| b.as_ref().to_vec()).unwrap_or_default())
    }

    async fn generate_random(&self, number_of_bytes: i32) -> Result<Vec<u8>, AwsServiceError> {
        let output = self
            .generate_random()
            .number_of_bytes(number_of_bytes)
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Kms, &e))?;
        Ok(output.plaintext().map(|b| b.as_ref().to_vec()).unwrap_or_default())
    }
}
