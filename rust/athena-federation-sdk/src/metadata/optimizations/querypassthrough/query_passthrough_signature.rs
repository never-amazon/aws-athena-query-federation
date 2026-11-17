use std::collections::HashMap;

use indexmap::IndexMap;

use super::super::OptimizationSubType;
use super::QueryPassthrough;
use crate::exceptions::{FederationError, Result};

pub const SCHEMA_FUNCTION_NAME: &str = "schemaFunctionName";
pub const ENABLE_QUERY_PASSTHROUGH: &str = "enable_query_passthrough";
pub const DEFAULT_ENABLE_QUERY_PASSTHROUGH_STATE: &str = "true";

pub trait QueryPassthroughSignature {
    fn get_function_schema(&self) -> String;

    fn get_function_name(&self) -> String;

    fn get_function_arguments(&self) -> Vec<String>;

    fn get_function_signature(&self) -> String {
        format!(
            "{}.{}",
            self.get_function_schema().to_uppercase(),
            self.get_function_name().to_uppercase()
        )
    }

    fn verify(&self, engine_qpt_arguments: &HashMap<String, Option<String>>) -> Result<()> {
        if !self.verify_function_signature(engine_qpt_arguments)? {
            return Err(FederationError::invalid_input("Function Signature doesn't match implementation's"));
        }
        for argument in self.get_function_arguments() {
            match engine_qpt_arguments.get(&argument) {
                None => {
                    return Err(FederationError::invalid_input(format!(
                        "Missing Query Passthrough Argument: {}",
                        argument
                    )))
                }
                Some(v) if v.as_deref().unwrap_or_default().is_empty() => {
                    return Err(FederationError::invalid_input(format!(
                        "Missing Query Passthrough Value for Argument: {}",
                        argument
                    )))
                }
                Some(_) => {}
            }
        }
        self.custom_connector_verifications(engine_qpt_arguments)
    }

    fn custom_connector_verifications(&self, _engine_qpt_arguments: &HashMap<String, Option<String>>) -> Result<()> {
        Ok(())
    }

    fn verify_function_signature(&self, argument_values: &HashMap<String, Option<String>>) -> Result<bool> {
        if let Some(received_signature) = argument_values.get(SCHEMA_FUNCTION_NAME) {
            tracing::info!("Found signature: {}", received_signature.as_deref().unwrap_or("null"));
            let received_signature = received_signature.as_deref().ok_or_else(|| {
                FederationError::NullPointer(
                    "Cannot invoke \"String.equalsIgnoreCase(String)\" because \"receivedSignature\" is null".into(),
                )
            })?;
            return Ok(received_signature.to_lowercase() == self.get_function_signature().to_lowercase());
        }
        tracing::warn!("No matching function signature found: {}", self.get_function_signature());
        Ok(false)
    }

    fn get_query_passthrough_capabilities(&self) -> Vec<OptimizationSubType> {
        vec![
            QueryPassthrough::QueryPassthroughSchema.with_schema(&self.get_function_schema()),
            QueryPassthrough::QueryPassthroughName.with_name(&self.get_function_name()),
            QueryPassthrough::QueryPassthroughArguments.with_arguments(self.get_function_arguments()),
        ]
    }

    fn add_query_passthrough_capability_if_enabled(
        &self,
        capabilities: &mut IndexMap<String, Vec<OptimizationSubType>>,
        config_options: &HashMap<String, String>,
    ) {
        if self.allow_query_passthrough(config_options) {
            tracing::info!("Query Passthrough is enabled; adding implementation to connector's capabilities");
            capabilities.insert(self.get_function_signature(), self.get_query_passthrough_capabilities());
        } else {
            tracing::info!("Query Passthrough is disabled");
        }
    }

    fn allow_query_passthrough(&self, config_options: &HashMap<String, String>) -> bool {
        config_options
            .get(ENABLE_QUERY_PASSTHROUGH)
            .map(|s| s.as_str())
            .unwrap_or(DEFAULT_ENABLE_QUERY_PASSTHROUGH_STATE)
            .to_lowercase()
            == "true"
    }
}
