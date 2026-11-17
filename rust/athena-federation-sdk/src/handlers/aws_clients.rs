use std::sync::Arc;

use aws_config::BehaviorVersion;

use crate::data::S3Client;
use crate::exceptions::Result;
use crate::security::{KmsClient, KmsEncryptionProvider, SecretsManagerClient};
use crate::AthenaClient;

#[derive(Clone)]
pub struct AwsClients {
    pub secrets_manager: Arc<dyn SecretsManagerClient>,
    pub athena: Arc<dyn AthenaClient>,
    pub s3: Arc<dyn S3Client>,
    pub kms: Arc<dyn KmsClient>,
    pub kms_encryption_provider: Arc<KmsEncryptionProvider>,
}

impl AwsClients {
    pub async fn from_environment() -> Result<AwsClients> {
        let sdk_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let kms = aws_sdk_kms::Client::new(&sdk_config);
        Ok(AwsClients {
            secrets_manager: Arc::new(aws_sdk_secretsmanager::Client::new(&sdk_config)),
            athena: Arc::new(aws_sdk_athena::Client::new(&sdk_config)),
            s3: Arc::new(aws_sdk_s3::Client::new(&sdk_config)),
            kms: Arc::new(kms.clone()),
            kms_encryption_provider: Arc::new(KmsEncryptionProvider::new(kms)?),
        })
    }
}
