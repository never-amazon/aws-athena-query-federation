use std::collections::HashMap;

use indexmap::IndexMap;

use super::super::OptimizationSubType;
use super::QueryPassthrough;
use crate::exceptions::{FederationError, Result};

pub const SCHEMA_FUNCTION_NAME: &str = "schemaFunctionName";
pub const ENABLE_QUERY_PASSTHROUGH: &str = "enable_query_passthrough";
pub const DEFAULT_ENABLE_QUERY_PASSTHROUGH_STATE: &str = "true";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryPassthroughSignature {
    pub schema: &'static str,
    pub name: &'static str,
    pub arguments: &'static [&'static str],
}

impl QueryPassthroughSignature {
    pub fn function_signature(&self) -> String {
        format!(
            "{}.{}",
            self.schema.to_uppercase(),
            self.name.to_uppercase()
        )
    }

    pub fn verify(&self, engine_qpt_arguments: &HashMap<String, Option<String>>) -> Result<()> {
        if !self.verify_function_signature(engine_qpt_arguments)? {
            return Err(FederationError::invalid_input(
                "Function Signature doesn't match implementation's",
            ));
        }
        for argument in self.arguments {
            match engine_qpt_arguments.get(*argument) {
                None => {
                    return Err(FederationError::invalid_input(format!(
                        "Missing Query Passthrough Argument: {argument}"
                    )))
                }
                Some(v) if v.as_deref().unwrap_or_default().is_empty() => {
                    return Err(FederationError::invalid_input(format!(
                        "Missing Query Passthrough Value for Argument: {argument}"
                    )))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn verify_function_signature(
        &self,
        argument_values: &HashMap<String, Option<String>>,
    ) -> Result<bool> {
        if let Some(received_signature) = argument_values.get(SCHEMA_FUNCTION_NAME) {
            tracing::info!(
                "Found signature: {}",
                received_signature.as_deref().unwrap_or("null")
            );
            let received_signature = received_signature.as_deref().ok_or_else(|| {
                FederationError::null_pointer("Cannot invoke \"String.equalsIgnoreCase(String)\" because \"receivedSignature\" is null")
            })?;
            return Ok(
                received_signature.to_lowercase() == self.function_signature().to_lowercase()
            );
        }
        tracing::warn!(
            "No matching function signature found: {}",
            self.function_signature()
        );
        Ok(false)
    }

    fn capabilities(&self) -> Vec<OptimizationSubType> {
        vec![
            QueryPassthrough::QueryPassthroughSchema.with_schema(self.schema),
            QueryPassthrough::QueryPassthroughName.with_name(self.name),
            QueryPassthrough::QueryPassthroughArguments
                .with_arguments(self.arguments.iter().map(ToString::to_string).collect()),
        ]
    }

    pub fn add_query_passthrough_capability_if_enabled(
        &self,
        capabilities: &mut IndexMap<String, Vec<OptimizationSubType>>,
        config_options: &HashMap<String, String>,
    ) {
        if allow_query_passthrough(config_options) {
            tracing::info!(
                "Query Passthrough is enabled; adding implementation to connector's capabilities"
            );
            capabilities.insert(self.function_signature(), self.capabilities());
        } else {
            tracing::info!("Query Passthrough is disabled");
        }
    }
}

fn allow_query_passthrough(config_options: &HashMap<String, String>) -> bool {
    config_options
        .get(ENABLE_QUERY_PASSTHROUGH)
        .map_or(DEFAULT_ENABLE_QUERY_PASSTHROUGH_STATE, String::as_str)
        .to_lowercase()
        == "true"
}
