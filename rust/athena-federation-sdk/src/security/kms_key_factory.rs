use std::sync::Arc;

use async_trait::async_trait;
use aws_sdk_kms::types::DataKeySpec;

use super::aes_gcm_block_crypto::NONCE_BYTES;
use super::aws_request_override_configuration::AwsRequestOverrideConfiguration;
use super::encryption_key::EncryptionKey;
use super::encryption_key_factory::EncryptionKeyFactory;
use super::kms_client::KmsClient;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

const NOT_FOUND_EXCEPTION: &str = "NotFoundException";

pub struct KmsKeyFactory {
    kms_client: Arc<dyn KmsClient>,
    master_key_id: String,
}

impl KmsKeyFactory {
    pub fn new(kms_client: Arc<dyn KmsClient>, master_key_id: impl Into<String>) -> KmsKeyFactory {
        KmsKeyFactory {
            kms_client,
            master_key_id: master_key_id.into(),
        }
    }
}

#[async_trait]
impl EncryptionKeyFactory for KmsKeyFactory {
    async fn create(&self) -> Result<EncryptionKey> {
        self.create_with_override_configuration(None).await
    }

    async fn create_with_override_configuration(
        &self,
        aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<EncryptionKey> {
        if aws_request_override_configuration.is_some() {
            tracing::info!("Using AWS KMS Request Override Configuration:");
        }
        let key = self
            .kms_client
            .generate_data_key(&self.master_key_id, DataKeySpec::Aes128, aws_request_override_configuration)
            .await
            .map_err(|e| {
                if e.is_error_code(NOT_FOUND_EXCEPTION) {
                    FederationError::connector(FederationSourceErrorCode::EntityNotFoundException, e.message())
                } else {
                    FederationError::from(e)
                }
            })?;
        let nonce = self.kms_client.generate_random(NONCE_BYTES as i32).await?;
        Ok(EncryptionKey::new(key, nonce))
    }
}
