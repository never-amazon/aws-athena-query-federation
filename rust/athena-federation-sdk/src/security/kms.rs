use std::sync::Arc;

use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sdk_kms::types::DataKeySpec;
use base64::Engine;

use crate::exceptions::{FederationError, Result};
use crate::security::aws_crypto::{AwsCrypto, EsdkAwsCrypto};
use crate::security::aws_request_override_configuration::{
    send_with_override, AwsRequestOverrideConfiguration,
};
use crate::security::aws_service_error::{AwsService, AwsServiceError};
use crate::security::FasToken;

#[async_trait]
pub trait KmsClient: Send + Sync {
    async fn generate_data_key(
        &self,
        key_id: &str,
        key_spec: DataKeySpec,
        override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Vec<u8>, AwsServiceError>;

    async fn generate_random(&self, number_of_bytes: usize) -> Result<Vec<u8>, AwsServiceError>;
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
        let output = send_with_override!(
            builder,
            override_configuration,
            aws_sdk_kms::config::Builder
        )
        .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Kms, &e))?;
        Ok(output
            .plaintext()
            .map(|b| b.as_ref().to_vec())
            .unwrap_or_default())
    }

    async fn generate_random(&self, number_of_bytes: usize) -> Result<Vec<u8>, AwsServiceError> {
        let output = self
            .generate_random()
            .number_of_bytes(i32::try_from(number_of_bytes).unwrap_or(i32::MAX))
            .send()
            .await
            .map_err(|e| AwsServiceError::from_sdk_error(AwsService::Kms, &e))?;
        Ok(output
            .plaintext()
            .map(|b| b.as_ref().to_vec())
            .unwrap_or_default())
    }
}

pub struct KmsEncryptionProvider {
    aws_crypto: Arc<dyn AwsCrypto>,
}

impl KmsEncryptionProvider {
    pub fn new(kms_client: aws_sdk_kms::Client) -> Result<KmsEncryptionProvider> {
        Ok(KmsEncryptionProvider {
            aws_crypto: Arc::new(EsdkAwsCrypto::new(kms_client)?),
        })
    }

    pub fn with_aws_crypto(aws_crypto: Arc<dyn AwsCrypto>) -> KmsEncryptionProvider {
        KmsEncryptionProvider { aws_crypto }
    }

    pub async fn get_fas_credentials(
        &self,
        kms_key_id: &str,
        fas_token: &str,
    ) -> Result<Credentials> {
        let fas_string = self.decrypt_data(kms_key_id, fas_token).await?;
        let credentials: FasToken = serde_json::from_str(&fas_string)
            .map_err(|e| FederationError::runtime_from(e.to_string(), e))?;
        Ok(Credentials::new(
            credentials.access_token(),
            credentials.secret_token(),
            Some(credentials.security_token().to_string()),
            None,
            "FASToken",
        ))
    }

    pub async fn decrypt_data(&self, kms_key_id: &str, data: &str) -> Result<String> {
        let decode = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| FederationError::IllegalArgument(e.to_string()))?;
        let result = self.aws_crypto.decrypt_data(kms_key_id, &decode).await?;
        Ok(String::from_utf8_lossy(&result).into_owned())
    }
}
