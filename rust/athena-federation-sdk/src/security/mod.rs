mod aws_crypto;
mod aws_request_override_configuration;
mod aws_service_error;
mod block_crypto;
mod encryption_key;
mod federated_identity;
mod kms;
mod secrets_manager;

pub use aws_crypto::{AwsCrypto, EsdkAwsCrypto};
pub(crate) use aws_request_override_configuration::send_with_override;
pub use aws_request_override_configuration::{
    AwsRequestOverrideConfiguration, AwsRequestOverrideConfigurationBuilder,
};
pub use aws_service_error::{AwsService, AwsServiceError};
pub(crate) use block_crypto::run_blocking;
pub use block_crypto::{BlockCrypto, ALGO, GCM_TAG_LENGTH_BITS, KEYSPEC, KEY_BYTES, NONCE_BYTES};
pub use encryption_key::{EncryptionKey, KeySource};
pub use federated_identity::{FasToken, FederatedIdentity};
pub use kms::{KmsClient, KmsEncryptionProvider};
pub use secrets_manager::{CachableSecretsManager, SecretsManagerClient};
