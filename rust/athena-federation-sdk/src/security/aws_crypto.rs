use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use aws_esdk::material_providers::client as mpl_client;
use aws_esdk::material_providers::types::keyring::KeyringRef;
use aws_esdk::material_providers::types::material_providers_config::MaterialProvidersConfig;
use aws_esdk::types::aws_encryption_sdk_config::AwsEncryptionSdkConfig;

use crate::exceptions::{FederationError, Result};

const CACHE_DURATION: Duration = Duration::from_hours(24);
const MAX_CACHE_SIZE: usize = 100;

#[async_trait]
pub trait AwsCrypto: Send + Sync {
    async fn decrypt_data(&self, kms_key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>>;
}

pub struct EsdkAwsCrypto {
    kms_client: aws_sdk_kms::Client,
    esdk_client: aws_esdk::Client,
    mpl: mpl_client::Client,
    keyrings: Mutex<HashMap<String, (Instant, KeyringRef)>>,
}

fn esdk_error(e: impl std::fmt::Debug) -> FederationError {
    FederationError::runtime(format!("{e:?}"))
}

impl EsdkAwsCrypto {
    pub fn new(kms_client: aws_sdk_kms::Client) -> Result<EsdkAwsCrypto> {
        let esdk_config = AwsEncryptionSdkConfig::builder()
            .build()
            .map_err(esdk_error)?;
        let esdk_client = aws_esdk::Client::from_conf(esdk_config).map_err(esdk_error)?;
        let mpl_config = MaterialProvidersConfig::builder()
            .build()
            .map_err(esdk_error)?;
        let mpl = mpl_client::Client::from_conf(mpl_config).map_err(esdk_error)?;
        Ok(EsdkAwsCrypto {
            kms_client,
            esdk_client,
            mpl,
            keyrings: Mutex::new(HashMap::new()),
        })
    }

    fn cached_keyring(&self, kms_key_id: &str) -> Option<KeyringRef> {
        let mut keyrings = self
            .keyrings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        keyrings.retain(|_, (created, _)| created.elapsed() < CACHE_DURATION);
        keyrings.get(kms_key_id).map(|(_, k)| k.clone())
    }

    async fn keyring(&self, kms_key_id: &str) -> Result<KeyringRef> {
        if let Some(keyring) = self.cached_keyring(kms_key_id) {
            return Ok(keyring);
        }
        let keyring = self
            .mpl
            .create_aws_kms_keyring()
            .kms_key_id(kms_key_id)
            .kms_client(self.kms_client.clone())
            .send()
            .await
            .map_err(esdk_error)?;
        let mut keyrings = self
            .keyrings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if keyrings.len() >= MAX_CACHE_SIZE {
            if let Some(oldest) = keyrings
                .iter()
                .min_by_key(|(_, (t, _))| *t)
                .map(|(k, _)| k.clone())
            {
                keyrings.remove(&oldest);
            }
        }
        keyrings.insert(kms_key_id.to_string(), (Instant::now(), keyring.clone()));
        Ok(keyring)
    }
}

#[async_trait]
impl AwsCrypto for EsdkAwsCrypto {
    async fn decrypt_data(&self, kms_key_id: &str, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let keyring = self.keyring(kms_key_id).await?;
        let output = self
            .esdk_client
            .decrypt()
            .ciphertext(aws_smithy_types::Blob::new(ciphertext))
            .keyring(keyring)
            .send()
            .await
            .map_err(esdk_error)?;
        Ok(output
            .plaintext
            .map(aws_smithy_types::Blob::into_inner)
            .unwrap_or_default())
    }
}
