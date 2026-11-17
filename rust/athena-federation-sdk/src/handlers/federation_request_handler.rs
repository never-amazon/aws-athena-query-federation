use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use aws_credential_types::Credentials;

use crate::connection::EnvironmentConstants;
use crate::credentials::{CredentialsProvider, DefaultCredentialsProvider};
use crate::data::S3Client;
use crate::exceptions::{FederationError, Result};
use crate::request::FederationRequest;
use crate::security::{
    AwsRequestOverrideConfiguration, CachableSecretsManager, FederatedIdentity,
    KmsEncryptionProvider,
};
use crate::AthenaClient;

pub type ConfigOptions = HashMap<String, Option<String>>;

#[async_trait]
pub trait FederationRequestHandler: Send + Sync {
    fn cachable_secrets_manager(&self) -> &CachableSecretsManager;

    fn kms_encryption_provider(&self) -> &KmsEncryptionProvider;

    async fn resolve_secrets(&self, raw_string: Option<&str>) -> Result<Option<String>> {
        self.cachable_secrets_manager()
            .resolve_secrets(raw_string)
            .await
    }

    async fn resolve_with_default_credentials(
        &self,
        raw_string: Option<&str>,
    ) -> Result<Option<String>> {
        self.cachable_secrets_manager()
            .resolve_with_default_credentials(raw_string)
            .await
    }

    async fn get_secret(&self, secret_name: &str) -> Result<String> {
        self.cachable_secrets_manager()
            .get_secret(secret_name)
            .await
    }

    async fn get_secret_with_request_override_configuration(
        &self,
        secret_name: &str,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<String> {
        self.cachable_secrets_manager()
            .get_secret_with_override_configuration(secret_name, request_override_configuration)
            .await
    }

    async fn get_session_credentials(
        &self,
        kms_key_id: &str,
        token_string: &str,
        kms_encryption_provider: &KmsEncryptionProvider,
    ) -> Result<Credentials> {
        kms_encryption_provider
            .get_fas_credentials(kms_key_id, token_string)
            .await
    }

    async fn get_request_override_config(
        &self,
        request: &FederationRequest,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        if self.is_request_federated(request) {
            let options = request.identity().map(FederatedIdentity::config_options);
            if fas_token(options).is_some() {
                return self
                    .get_request_override_config_with_config_options(options)
                    .await;
            }
        }
        Ok(None)
    }

    async fn get_request_override_config_with_config_options(
        &self,
        config_options: Option<&ConfigOptions>,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        self.get_request_override_config_with_kms_encryption_provider(
            config_options,
            self.kms_encryption_provider(),
        )
        .await
    }

    async fn get_request_override_config_with_kms_encryption_provider(
        &self,
        config_options: Option<&ConfigOptions>,
        kms_encryption_provider: &KmsEncryptionProvider,
    ) -> Result<Option<AwsRequestOverrideConfiguration>> {
        let Some(options) = config_options else {
            return Ok(None);
        };
        let (Some(token), Some(kms_key)) = (
            options.get(EnvironmentConstants::FAS_TOKEN),
            options.get(EnvironmentConstants::SERVICE_KMS),
        ) else {
            return Ok(None);
        };
        let kms_key = kms_key.as_deref().ok_or_else(null_fas_option)?;
        let token = token.as_deref().ok_or_else(null_fas_option)?;
        let credentials = self
            .get_session_credentials(kms_key, token, kms_encryption_provider)
            .await?;
        Ok(Some(
            AwsRequestOverrideConfiguration::builder()
                .credentials_provider(credentials)
                .build(),
        ))
    }

    fn get_s3_client(
        &self,
        aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
        default_s3: &Arc<dyn S3Client>,
    ) -> Arc<dyn S3Client> {
        aws_request_override_configuration
            .and_then(AwsRequestOverrideConfiguration::credentials)
            .map_or_else(
                || Arc::clone(default_s3),
                |credentials| default_s3.with_credentials(credentials),
            )
    }

    fn get_athena_client(
        &self,
        aws_request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
        default_athena: &Arc<dyn AthenaClient>,
    ) -> Arc<dyn AthenaClient> {
        aws_request_override_configuration
            .and_then(AwsRequestOverrideConfiguration::credentials)
            .map_or_else(
                || Arc::clone(default_athena),
                |credentials| default_athena.with_credentials(credentials),
            )
    }

    fn is_request_federated(&self, request: &FederationRequest) -> bool {
        fas_token(request.identity().map(FederatedIdentity::config_options)).is_some()
    }

    async fn get_credential_provider(&self) -> Result<Option<Box<dyn CredentialsProvider>>> {
        self.get_credential_provider_with_request_override_configuration(None)
            .await
    }

    async fn get_credential_provider_with_request_override_configuration(
        &self,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Option<Box<dyn CredentialsProvider>>> {
        match self.database_connection_secret() {
            Some(secret_name) if !secret_name.trim().is_empty() => {
                tracing::info!("Using Secrets Manager.");
                self.create_credentials_provider(secret_name, request_override_configuration)
                    .await
                    .map(Some)
            }
            _ => Ok(None),
        }
    }

    async fn create_credentials_provider(
        &self,
        secret_name: &str,
        request_override_configuration: Option<&AwsRequestOverrideConfiguration>,
    ) -> Result<Box<dyn CredentialsProvider>> {
        let secret = self
            .get_secret_with_request_override_configuration(
                secret_name,
                request_override_configuration,
            )
            .await?;
        Ok(Box::new(DefaultCredentialsProvider::new(&secret)?))
    }

    fn database_connection_secret(&self) -> Option<&str> {
        None
    }
}

fn fas_token(config_options: Option<&ConfigOptions>) -> Option<&str> {
    config_options?
        .get(EnvironmentConstants::FAS_TOKEN)?
        .as_deref()
}

fn null_fas_option() -> FederationError {
    FederationError::null_pointer(
        "Cannot invoke \"String.getBytes(java.nio.charset.Charset)\" because \"src\" is null",
    )
}
