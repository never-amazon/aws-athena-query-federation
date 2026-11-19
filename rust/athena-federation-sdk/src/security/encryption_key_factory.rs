use std::sync::Arc;

use aws_sdk_kms::types::DataKeySpec;

use super::aes_gcm_block_crypto::{KEY_BYTES, NONCE_BYTES};
use super::aws_request_override_configuration::AwsRequestOverrideConfiguration;
use super::encryption_key::EncryptionKey;
use super::kms_client::KmsClient;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

const NOT_FOUND_EXCEPTION: &str = "NotFoundException";

#[derive(Clone)]
pub enum KeySource {
    Local,
    Kms {
        client: Arc<dyn KmsClient>,
        master_key_id: String,
    },
}

impl KeySource {
    pub fn kms(client: Arc<dyn KmsClient>, master_key_id: impl Into<String>) -> KeySource {
        KeySource::Kms {
            client,
            master_key_id: master_key_id.into(),
        }
    }

    pub async fn create(
        &self,
        aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<EncryptionKey> {
        match self {
            KeySource::Local => local_key(),
            KeySource::Kms {
                client,
                master_key_id,
            } => {
                kms_key(
                    client.as_ref(),
                    master_key_id,
                    aws_request_override_configuration,
                )
                .await
            }
        }
    }
}

fn local_key() -> Result<EncryptionKey> {
    let mut key = vec![0u8; KEY_BYTES];
    let mut nonce = vec![0u8; NONCE_BYTES];
    getrandom::fill(&mut key)
        .and_then(|()| getrandom::fill(&mut nonce))
        .map_err(|e| FederationError::internal(e.to_string()))?;
    Ok(EncryptionKey::new(key, nonce))
}

async fn kms_key(
    kms_client: &dyn KmsClient,
    master_key_id: &str,
    aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
) -> Result<EncryptionKey> {
    if aws_request_override_configuration.is_some() {
        tracing::info!("Using AWS KMS Request Override Configuration:");
    }
    let key = kms_client
        .generate_data_key(
            master_key_id,
            DataKeySpec::Aes128,
            aws_request_override_configuration,
        )
        .await
        .map_err(|e| {
            if e.is_error_code(NOT_FOUND_EXCEPTION) {
                FederationError::connector(
                    FederationSourceErrorCode::EntityNotFoundException,
                    e.get_message().unwrap_or_default(),
                )
            } else {
                FederationError::from(e)
            }
        })?;
    let nonce_bytes =
        i32::try_from(NONCE_BYTES).map_err(|e| FederationError::internal(e.to_string()))?;
    let nonce = kms_client.generate_random(nonce_bytes).await?;
    Ok(EncryptionKey::new(key, nonce))
}
