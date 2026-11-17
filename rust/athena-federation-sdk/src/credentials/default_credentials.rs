use std::any::Any;
use std::collections::HashMap;

use super::credentials::Credentials;
use super::credentials_constants::CredentialsConstants;
use crate::exceptions::{FederationError, Result};

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
}

impl Credentials for DefaultCredentials {
    fn properties(&self) -> HashMap<String, String> {
        HashMap::from([
            (CredentialsConstants::USER.to_string(), self.user.clone()),
            (
                CredentialsConstants::PASSWORD.to_string(),
                self.password.clone(),
            ),
        ])
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
