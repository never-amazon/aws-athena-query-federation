use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FederatedIdentity {
    arn: Option<String>,
    account: Option<String>,
    principal_tags: HashMap<String, Option<String>>,
    iam_groups: Vec<Option<String>>,
    config_options: HashMap<String, Option<String>>,
}

impl FederatedIdentity {
    pub fn new(
        arn: impl Into<String>,
        account: impl Into<String>,
        principal_tags: HashMap<String, Option<String>>,
        iam_groups: Vec<Option<String>>,
        config_options: HashMap<String, Option<String>>,
    ) -> FederatedIdentity {
        FederatedIdentity::new_nullable(
            Some(arn.into()),
            Some(account.into()),
            principal_tags,
            iam_groups,
            config_options,
        )
    }

    pub fn new_nullable(
        arn: Option<String>,
        account: Option<String>,
        principal_tags: HashMap<String, Option<String>>,
        iam_groups: Vec<Option<String>>,
        config_options: HashMap<String, Option<String>>,
    ) -> FederatedIdentity {
        FederatedIdentity {
            arn,
            account,
            principal_tags,
            iam_groups,
            config_options,
        }
    }

    pub fn arn(&self) -> Option<&str> {
        self.arn.as_deref()
    }

    pub fn account(&self) -> Option<&str> {
        self.account.as_deref()
    }

    pub fn principal_tags(&self) -> &HashMap<String, Option<String>> {
        &self.principal_tags
    }

    pub fn iam_groups(&self) -> &[Option<String>] {
        &self.iam_groups
    }

    pub fn config_options(&self) -> &HashMap<String, Option<String>> {
        &self.config_options
    }
}

impl fmt::Display for FederatedIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FederatedIdentity{{arn={}, account={}}}",
            self.arn.as_deref().unwrap_or("null"),
            self.account.as_deref().unwrap_or("null")
        )
    }
}

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

    pub fn access_token(&self) -> &str {
        &self.access
    }

    pub fn secret_token(&self) -> &str {
        &self.secret
    }

    pub fn security_token(&self) -> &str {
        &self.security
    }
}
