use std::sync::Arc;

use aws_credential_types::Credentials;
use base64::Engine;

use super::aws_crypto::{AwsCrypto, EsdkAwsCrypto};
use super::fas_token::FasToken;
use crate::exceptions::{FederationError, Result};

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

    pub async fn get_fas_credentials(&self, kms_key_id: &str, fas_token: &str) -> Result<Credentials> {
        let fas_string = self.decrypt_data(kms_key_id, fas_token).await?;
        let credentials: FasToken =
            serde_json::from_str(&fas_string).map_err(|e| FederationError::Runtime(e.to_string()))?;
        Ok(Credentials::new(
            credentials.get_access_token(),
            credentials.get_secret_token(),
            Some(credentials.get_security_token().to_string()),
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
