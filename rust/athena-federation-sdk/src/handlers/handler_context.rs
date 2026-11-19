use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use aws_credential_types::Credentials;

use super::athena_exception_filter::is_athena_throttle;
use super::aws_clients::AwsClients;
use crate::connection::environment_constants;
use crate::credentials::{CredentialsProvider, DefaultCredentialsProvider};
use crate::data::spill_config::SpillConfig;
use crate::data::S3Client;
use crate::exceptions::{FederationError, Result};
use crate::records::ReadRecordsRequest;
use crate::request::FederationRequest;
use crate::security::{
    AwsRequestOverrideConfiguration, AwsServiceError, CachableSecretsManager, FederatedIdentity,
    KmsEncryptionProvider,
};
use crate::{AthenaClient, QueryStatusChecker, ThrottleConfig, ThrottlingInvoker};

pub type ConfigOptions = HashMap<String, Option<String>>;

const MAX_BLOCK_SIZE_BYTES: &str = "MAX_BLOCK_SIZE_BYTES";
const NUM_SPILL_THREADS: NonZeroUsize = NonZeroUsize::new(2).unwrap();

pub struct HandlerContext {
    source_type: String,
    config_options: HashMap<String, String>,
    secrets_manager: CachableSecretsManager,
    kms_encryption_provider: Arc<KmsEncryptionProvider>,
    athena: Arc<dyn AthenaClient>,
    s3: Arc<dyn S3Client>,
    athena_invoker: Arc<ThrottlingInvoker<AwsServiceError>>,
}

impl HandlerContext {
    pub fn new(
        source_type: impl Into<String>,
        clients: AwsClients,
        config_options: HashMap<String, String>,
    ) -> Result<HandlerContext> {
        let athena_invoker = ThrottlingInvoker::new(
            ThrottleConfig::from_config_options(&config_options)?,
            is_athena_throttle,
            None,
        )?;
        Ok(HandlerContext {
            source_type: source_type.into(),
            config_options,
            secrets_manager: CachableSecretsManager::new(clients.secrets_manager),
            kms_encryption_provider: clients.kms_encryption_provider,
            athena: clients.athena,
            s3: clients.s3,
            athena_invoker: Arc::new(athena_invoker),
        })
    }

    pub async fn from_environment(
        source_type: impl Into<String>,
        config_options: HashMap<String, String>,
    ) -> Result<HandlerContext> {
        HandlerContext::new(
            source_type,
            AwsClients::from_environment().await?,
            config_options,
        )
    }

    pub fn source_type(&self) -> &str {
        &self.source_type
    }

    pub fn config_options(&self) -> &HashMap<String, String> {
        &self.config_options
    }

    pub fn secrets_manager(&self) -> &CachableSecretsManager {
        &self.secrets_manager
    }

    pub fn default_s3(&self) -> &Arc<dyn S3Client> {
        &self.s3
    }

    pub fn default_athena(&self) -> &Arc<dyn AthenaClient> {
        &self.athena
    }

    pub async fn session_credentials(&self, kms_key_id: &str, token: &str) -> Result<Credentials> {
        self.kms_encryption_provider
            .get_fas_credentials(kms_key_id, token)
            .await
    }

    pub async fn request_override_config(
        &self,
        config_options: Option<&ConfigOptions>,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        let Some(options) = config_options else {
            return Ok(None);
        };
        let (Some(token), Some(kms_key)) = (
            options.get(environment_constants::FAS_TOKEN),
            options.get(environment_constants::SERVICE_KMS),
        ) else {
            return Ok(None);
        };
        let kms_key = kms_key.as_deref().ok_or_else(null_fas_option)?;
        let token = token.as_deref().ok_or_else(null_fas_option)?;
        let credentials = self.session_credentials(kms_key, token).await?;
        Ok(Some(
            AwsRequestOverrideConfiguration::builder()
                .credentials_provider(credentials)
                .build(),
        ))
    }

    pub async fn identity_override_config(
        &self,
        identity: Option<&FederatedIdentity>,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        self.request_override_config(identity.map(FederatedIdentity::config_options))
            .await
    }

    pub async fn federated_override_config(
        &self,
        request: &FederationRequest,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        if request.is_federated() {
            self.identity_override_config(request.identity()).await
        } else {
            Ok(None)
        }
    }

    pub fn s3_client(
        &self,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Arc<dyn S3Client> {
        override_config
            .and_then(AwsRequestOverrideConfiguration::credentials)
            .map_or_else(
                || Arc::clone(&self.s3),
                |credentials| self.s3.with_credentials(credentials),
            )
    }

    pub fn athena_client(
        &self,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Arc<dyn AthenaClient> {
        override_config
            .and_then(AwsRequestOverrideConfiguration::credentials)
            .map_or_else(
                || Arc::clone(&self.athena),
                |credentials| self.athena.with_credentials(credentials),
            )
    }

    pub fn query_status_checker(
        &self,
        override_config: Option<&AwsRequestOverrideConfiguration>,
        query_id: impl Into<String>,
    ) -> QueryStatusChecker {
        QueryStatusChecker::new(
            self.athena_client(override_config),
            Arc::clone(&self.athena_invoker),
            query_id,
        )
    }

    pub async fn secret(
        &self,
        secret_name: &str,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<String> {
        self.secrets_manager
            .get_secret(secret_name, override_config)
            .await
    }

    pub async fn credentials_provider(
        &self,
        secret_name: Option<&str>,
        override_config: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        match secret_name {
            Some(secret_name) if !secret_name.trim().is_empty() => {
                tracing::info!("Using Secrets Manager.");
                let secret = self.secret(secret_name, override_config).await?;
                Ok(Some(Box::new(DefaultCredentialsProvider::new(&secret)?)))
            }
            _ => Ok(None),
        }
    }

    pub fn spill_config(&self, request: &ReadRecordsRequest) -> Result<SpillConfig> {
        let mut max_block_size = request.max_block_size();
        if let Some(value) = self.config_options.get(MAX_BLOCK_SIZE_BYTES) {
            max_block_size = value.parse::<i64>().map_err(|_| {
                FederationError::NumberFormat(format!("For input string: \"{value}\""))
            })?;
        }
        let spill_location = request
            .split()
            .spill_location()
            .cloned()
            .ok_or_else(|| FederationError::null_pointer("spillLocation was null"))?;
        Ok(SpillConfig {
            encryption_key: request.split().encryption_key().cloned(),
            max_block_bytes: max_block_size,
            max_inline_block_bytes: request.max_inline_block_size(),
            spill_threads: Some(NUM_SPILL_THREADS),
            ..SpillConfig::new(spill_location, request.query_id())
        })
    }
}

fn null_fas_option() -> FederationError {
    FederationError::null_pointer(
        "Cannot invoke \"String.getBytes(java.nio.charset.Charset)\" because \"src\" is null",
    )
}
