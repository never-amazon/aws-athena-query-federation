use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FasToken {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "secretToken")]
    secret_token: String,
    #[serde(rename = "securityToken")]
    security_token: String,
}

impl FasToken {
    pub fn new(
        access_token: impl Into<String>,
        secret_token: impl Into<String>,
        security_token: impl Into<String>,
    ) -> FasToken {
        FasToken {
            access_token: access_token.into(),
            secret_token: secret_token.into(),
            security_token: security_token.into(),
        }
    }

    pub fn get_access_token(&self) -> &str {
        &self.access_token
    }

    pub fn get_secret_token(&self) -> &str {
        &self.secret_token
    }

    pub fn get_security_token(&self) -> &str {
        &self.security_token
    }
}
