use std::fmt;

use athena_federation_sdk::{FederationError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowflakeAuthType {
    Snowflake,
    SnowflakeJwt,
    Oauth,
}

impl SnowflakeAuthType {
    pub fn values() -> &'static [SnowflakeAuthType] {
        &[
            SnowflakeAuthType::Snowflake,
            SnowflakeAuthType::SnowflakeJwt,
            SnowflakeAuthType::Oauth,
        ]
    }

    pub fn value(self) -> &'static str {
        match self {
            SnowflakeAuthType::Snowflake => "snowflake",
            SnowflakeAuthType::SnowflakeJwt => "snowflake_jwt",
            SnowflakeAuthType::Oauth => "oauth",
        }
    }

    pub fn from_string(auth_type: Option<&str>) -> Result<SnowflakeAuthType> {
        let Some(auth_type) = auth_type else {
            return Err(FederationError::illegal_argument(
                "Auth type cannot be null",
            ));
        };
        Self::values()
            .iter()
            .copied()
            .find(|candidate| candidate.value() == auth_type)
            .ok_or_else(|| {
                FederationError::illegal_argument(format!(
                    "Unsupported authentication type: {auth_type}"
                ))
            })
    }
}

impl fmt::Display for SnowflakeAuthType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.value())
    }
}
