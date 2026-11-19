use std::collections::HashMap;

use super::default_credentials::DefaultCredentials;
use super::oauth_access_token_credentials::OAuthAccessTokenCredentials;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credentials {
    UserPassword(DefaultCredentials),
    AccessToken(OAuthAccessTokenCredentials),
}

impl Credentials {
    pub fn properties(&self) -> HashMap<String, String> {
        match self {
            Credentials::UserPassword(c) => c.properties(),
            Credentials::AccessToken(c) => c.properties(),
        }
    }
}

impl From<DefaultCredentials> for Credentials {
    fn from(credentials: DefaultCredentials) -> Credentials {
        Credentials::UserPassword(credentials)
    }
}

impl From<OAuthAccessTokenCredentials> for Credentials {
    fn from(credentials: OAuthAccessTokenCredentials) -> Credentials {
        Credentials::AccessToken(credentials)
    }
}
