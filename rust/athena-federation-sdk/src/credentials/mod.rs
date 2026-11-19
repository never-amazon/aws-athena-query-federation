mod cross_account;
mod http_client;
mod oauth;
mod provider;

pub use cross_account::get_cross_account_credentials_if_present;
pub use http_client::{
    HttpClient, HttpClientError, HttpRequest, HttpRequestBuilder, HttpResponse, ReqwestHttpClient,
};
pub use oauth::{OAuthCredentialsProvider, OAuthFlow};
pub use provider::{
    create_credential_provider, DefaultCredentialsProvider, StaticCredentialsProvider,
};

use std::any::Any;
use std::collections::HashMap;

use async_trait::async_trait;

use crate::exceptions::{FederationError, Result};

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

pub const USERNAME: &str = "username";
pub const USER: &str = "user";
pub const PASSWORD: &str = "password";
pub const ACCESS_TOKEN: &str = "access_token";
pub const EXPIRES_IN: &str = "expires_in";
pub const FETCHED_AT: &str = "fetched_at";
pub const CLIENT_ID: &str = "client_id";
pub const CLIENT_SECRET: &str = "client_secret";
pub const ACCESS_TOKEN_PROPERTY: &str = "accessToken";

#[async_trait]
pub trait CredentialsProvider: Send + Sync {
    async fn get_credential(&self) -> Result<Credentials>;

    async fn get_credential_map(&self) -> Result<HashMap<String, String>> {
        Ok(self.get_credential().await?.properties())
    }

    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefaultCredentials {
    user: String,
    password: String,
}

fn not_blank(value: Option<&str>, message: &str) -> Result<String> {
    match value {
        None => Err(FederationError::null_pointer(message.to_string())),
        Some(v) if v.trim().is_empty() => {
            Err(FederationError::IllegalArgument(message.to_string()))
        }
        Some(v) => Ok(v.to_string()),
    }
}

impl DefaultCredentials {
    pub fn new(user: &str, password: &str) -> Result<DefaultCredentials> {
        DefaultCredentials::new_nullable(Some(user), Some(password))
    }

    pub fn new_nullable(user: Option<&str>, password: Option<&str>) -> Result<DefaultCredentials> {
        Ok(DefaultCredentials {
            user: not_blank(user, "User must not be blank")?,
            password: not_blank(password, "Password must not be blank")?,
        })
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn password(&self) -> &str {
        &self.password
    }

    pub fn properties(&self) -> HashMap<String, String> {
        HashMap::from([
            (USER.to_string(), self.user.clone()),
            (PASSWORD.to_string(), self.password.clone()),
        ])
    }
}

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
        HashMap::from([(ACCESS_TOKEN_PROPERTY.to_string(), self.access_token.clone())])
    }
}
