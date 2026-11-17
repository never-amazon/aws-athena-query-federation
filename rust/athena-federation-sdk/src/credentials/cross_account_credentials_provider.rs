use std::collections::HashMap;

use aws_config::default_provider::credentials::DefaultCredentialsChain;
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;

use crate::exceptions::{FederationError, Result};
use crate::security::{AwsService, AwsServiceError};

const CROSS_ACCOUNT_ROLE_ARN_CONFIG: &str = "cross_account_role_arn";

pub struct CrossAccountCredentialsProvider;

impl CrossAccountCredentialsProvider {
    pub async fn get_cross_account_credentials_if_present(
        config_options: &HashMap<String, String>,
        role_session_name: &str,
    ) -> Result<SharedCredentialsProvider> {
        if let Some(role_arn) = config_options.get(CROSS_ACCOUNT_ROLE_ARN_CONFIG) {
            tracing::debug!("Found cross-account role arn to assume.");
            let sdk_config = aws_config::defaults(BehaviorVersion::latest()).load().await;
            let sts_client = aws_sdk_sts::Client::new(&sdk_config);
            let response = sts_client
                .assume_role()
                .role_arn(role_arn)
                .role_session_name(role_session_name)
                .send()
                .await
                .map_err(|e| {
                    FederationError::from(AwsServiceError::from_sdk_error(AwsService::Sts, &e))
                })?;
            let credentials = response.credentials().ok_or_else(|| {
                FederationError::null_pointer("AssumeRole returned no credentials")
            })?;
            let session_credentials = Credentials::new(
                credentials.access_key_id(),
                credentials.secret_access_key(),
                Some(credentials.session_token().to_string()),
                None,
                "CrossAccountCredentialsProvider",
            );
            return Ok(SharedCredentialsProvider::new(session_credentials));
        }
        Ok(SharedCredentialsProvider::new(
            DefaultCredentialsChain::builder().build().await,
        ))
    }
}
