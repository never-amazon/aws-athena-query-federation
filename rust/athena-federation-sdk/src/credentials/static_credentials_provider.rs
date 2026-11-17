use std::any::Any;

use async_trait::async_trait;

use super::credentials::Credentials;
use super::credentials_provider::CredentialsProvider;
use super::default_credentials::DefaultCredentials;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticCredentialsProvider {
    default_credentials: DefaultCredentials,
}

impl StaticCredentialsProvider {
    pub fn new(default_credentials: DefaultCredentials) -> Result<StaticCredentialsProvider> {
        if default_credentials.get_user().trim().is_empty()
            || default_credentials.get_password().trim().is_empty()
        {
            return Err(FederationError::invalid_input(
                "User or password must not be blank.",
            ));
        }
        Ok(StaticCredentialsProvider {
            default_credentials,
        })
    }

    pub fn get_credential(&self) -> &DefaultCredentials {
        &self.default_credentials
    }
}

#[async_trait]
impl CredentialsProvider for StaticCredentialsProvider {
    async fn get_credential(&self) -> Result<Box<dyn Credentials>> {
        Ok(Box::new(self.default_credentials.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
