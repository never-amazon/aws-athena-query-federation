use std::collections::HashMap;

use super::credentials_constants::CredentialsConstants;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OAuthAccessTokenCredentials {
    access_token: String,
}

impl OAuthAccessTokenCredentials {
    pub fn new(access_token: &str) -> Result<OAuthAccessTokenCredentials> {
        if access_token.trim().is_empty() {
            return Err(FederationError::IllegalArgument(
                "Access token must not be blank".into(),
            ));
        }
        Ok(OAuthAccessTokenCredentials {
            access_token: access_token.to_string(),
        })
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn properties(&self) -> HashMap<String, String> {
        HashMap::from([(
            CredentialsConstants::ACCESS_TOKEN_PROPERTY.to_string(),
            self.access_token.clone(),
        )])
    }
}
