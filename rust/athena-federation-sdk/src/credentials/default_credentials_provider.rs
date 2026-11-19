use std::any::Any;
use std::collections::HashMap;

use async_trait::async_trait;

use super::credentials::Credentials;
use super::credentials_constants;
use super::credentials_provider::CredentialsProvider;
use super::default_credentials::DefaultCredentials;
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultCredentialsProvider {
    default_credentials: DefaultCredentials,
}

impl DefaultCredentialsProvider {
    pub fn new(secret_string: &str) -> Result<DefaultCredentialsProvider> {
        let original_map: HashMap<String, serde_json::Value> = serde_json::from_str(secret_string)
            .map_err(|e| {
                FederationError::connector_with_details(
                    FederationSourceErrorCode::InternalServiceException,
                    "Could not deserialize RDS credentials into HashMap: ",
                    e.to_string(),
                )
            })?;
        let mut rds_secrets: HashMap<String, Option<String>> = HashMap::new();
        for (key, value) in original_map {
            if key.eq_ignore_ascii_case(credentials_constants::USERNAME)
                || key.eq_ignore_ascii_case(credentials_constants::PASSWORD)
            {
                let value = match value {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) => Some(s),
                    other => {
                        return Err(FederationError::class_cast(
                            json_type_name(&other),
                            "java.lang.String",
                        ))
                    }
                };
                rds_secrets.insert(key.to_lowercase(), value);
            }
        }
        let user = rds_secrets
            .get(credentials_constants::USERNAME)
            .cloned()
            .flatten();
        let password = rds_secrets
            .get(credentials_constants::PASSWORD)
            .cloned()
            .flatten();
        Ok(DefaultCredentialsProvider {
            default_credentials: DefaultCredentials::new_nullable(
                user.as_deref(),
                password.as_deref(),
            )?,
        })
    }

    pub fn credential(&self) -> &DefaultCredentials {
        &self.default_credentials
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Bool(_) => "java.lang.Boolean",
        serde_json::Value::Number(n) if n.is_f64() => "java.lang.Double",
        serde_json::Value::Number(n) if n.as_i64().is_some_and(|v| i32::try_from(v).is_ok()) => {
            "java.lang.Integer"
        }
        serde_json::Value::Number(n) if n.is_i64() => "java.lang.Long",
        serde_json::Value::Number(_) => "java.math.BigInteger",
        serde_json::Value::Array(_) => "java.util.ArrayList",
        serde_json::Value::Object(_) => "java.util.LinkedHashMap",
        _ => "java.lang.Object",
    }
}

#[async_trait]
impl CredentialsProvider for DefaultCredentialsProvider {
    async fn get_credential(&self) -> Result<Credentials> {
        Ok(self.default_credentials.clone().into())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
