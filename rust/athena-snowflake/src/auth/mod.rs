mod ber;
mod credentials;
mod pbe;
mod pbes2;

use std::fmt;
use std::str::FromStr;

use athena_federation_sdk::FederationError;

pub use credentials::{SnowflakeAuthUtils, SnowflakePrivateKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnowflakeAuthType {
    Snowflake,
    SnowflakeJwt,
    Oauth,
}

impl SnowflakeAuthType {
    pub const fn as_str(self) -> &'static str {
        match self {
            SnowflakeAuthType::Snowflake => "snowflake",
            SnowflakeAuthType::SnowflakeJwt => "snowflake_jwt",
            SnowflakeAuthType::Oauth => "oauth",
        }
    }
}

impl FromStr for SnowflakeAuthType {
    type Err = FederationError;

    fn from_str(auth_type: &str) -> Result<SnowflakeAuthType, FederationError> {
        [
            SnowflakeAuthType::Snowflake,
            SnowflakeAuthType::SnowflakeJwt,
            SnowflakeAuthType::Oauth,
        ]
        .into_iter()
        .find(|candidate| candidate.as_str() == auth_type)
        .ok_or_else(|| {
            FederationError::illegal_argument(format!(
                "Unsupported authentication type: {auth_type}"
            ))
        })
    }
}

impl fmt::Display for SnowflakeAuthType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
