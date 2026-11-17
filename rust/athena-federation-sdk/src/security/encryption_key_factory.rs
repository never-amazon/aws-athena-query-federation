use async_trait::async_trait;

use super::aws_request_override_configuration::AwsRequestOverrideConfiguration;
use super::encryption_key::EncryptionKey;
use crate::exceptions::Result;

#[async_trait]
pub trait EncryptionKeyFactory: Send + Sync {
    async fn create(&self) -> Result<EncryptionKey>;

    async fn create_with_override_configuration(
        &self,
        _aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<EncryptionKey> {
        self.create().await
    }
}
