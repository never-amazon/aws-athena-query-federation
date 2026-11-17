use std::any::Any;
use std::collections::HashMap;

use async_trait::async_trait;

use super::credentials::Credentials;
use super::credentials_constants::CredentialsConstants;
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
            if key.eq_ignore_ascii_case(CredentialsConstants::USERNAME)
                || key.eq_ignore_ascii_case(CredentialsConstants::PASSWORD)
            {
                let value = match value {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) => Some(s),
                    other => {
                        return Err(FederationError::ClassCast(format!(
                            "class {} cannot be cast to class java.lang.String",
                            json_type_name(&other)
                        )))
                    }
                };
                rds_secrets.insert(key.to_lowercase(), value);
            }
        }
        let user = rds_secrets
            .get(CredentialsConstants::USERNAME)
            .cloned()
            .flatten();
        let password = rds_secrets
            .get(CredentialsConstants::PASSWORD)
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
        serde_json::Value::Number(_) => "java.lang.Integer",
        serde_json::Value::Array(_) => "java.util.ArrayList",
        serde_json::Value::Object(_) => "java.util.LinkedHashMap",
        _ => "java.lang.Object",
    }
}

#[async_trait]
impl CredentialsProvider for DefaultCredentialsProvider {
    async fn get_credential(&self) -> Result<Box<dyn Credentials>> {
        Ok(Box::new(self.default_credentials.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
