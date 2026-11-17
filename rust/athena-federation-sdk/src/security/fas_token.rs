use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FasToken {
    #[serde(rename = "accessToken")]
    access: String,
    #[serde(rename = "secretToken")]
    secret: String,
    #[serde(rename = "securityToken")]
    security: String,
}

impl FasToken {
    pub fn new(
        access_token: impl Into<String>,
        secret_token: impl Into<String>,
        security_token: impl Into<String>,
    ) -> FasToken {
        FasToken {
            access: access_token.into(),
            secret: secret_token.into(),
            security: security_token.into(),
        }
    }

    pub fn get_access_token(&self) -> &str {
        &self.access
    }

    pub fn get_secret_token(&self) -> &str {
        &self.secret
    }

    pub fn get_security_token(&self) -> &str {
        &self.security
    }
}
