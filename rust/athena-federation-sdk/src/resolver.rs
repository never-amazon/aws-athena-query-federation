use std::collections::HashMap;
use std::str::FromStr;

use crate::exceptions::{FederationError, Result};
use crate::named_enum::named_enum;

pub const CASING_MODE_CONFIGURATION_KEY: &str = "casing_mode";
pub const DEFAULT_GLUE_CONNECTION: &str = "glue_connection";

named_enum! {
    pub enum FederationSdkCasingMode {
        None => "NONE",
        Lower => "LOWER",
        Upper => "UPPER",
        CaseInsensitiveSearch => "CASE_INSENSITIVE_SEARCH",
        Annotation => "ANNOTATION",
    }
}

impl FromStr for FederationSdkCasingMode {
    type Err = FederationError;

    fn from_str(name: &str) -> Result<Self> {
        FederationSdkCasingMode::from_name(name).ok_or_else(|| {
            FederationError::IllegalArgument(format!(
                "No enum constant com.amazonaws.athena.connector.lambda.resolver.CaseResolver.FederationSDKCasingMode.{name}"
            ))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseResolver {
    pub source_type: String,
    pub non_glue_based_default_casing_mode: FederationSdkCasingMode,
    pub glue_connection_based_default_casing_mode: FederationSdkCasingMode,
}

impl CaseResolver {
    pub fn new(
        source_type: impl Into<String>,
        non_glue_based_default_casing_mode: FederationSdkCasingMode,
        glue_connection_based_default_casing_mode: FederationSdkCasingMode,
    ) -> CaseResolver {
        CaseResolver {
            source_type: source_type.into(),
            non_glue_based_default_casing_mode,
            glue_connection_based_default_casing_mode,
        }
    }

    pub fn get_casing_mode(
        &self,
        config_options: &HashMap<String, String>,
    ) -> Result<FederationSdkCasingMode> {
        let is_glue_connection = config_options
            .get(DEFAULT_GLUE_CONNECTION)
            .is_some_and(|v| !v.trim().is_empty());
        let Some(configured) = config_options.get(CASING_MODE_CONFIGURATION_KEY) else {
            let mode = if is_glue_connection {
                self.glue_connection_based_default_casing_mode
            } else {
                self.non_glue_based_default_casing_mode
            };
            tracing::debug!(
                "CASING MODE disable default casing mode for :'{}' is :'{}'",
                self.source_type,
                mode
            );
            return Ok(mode);
        };
        match configured.to_uppercase().parse::<FederationSdkCasingMode>() {
            Ok(mode) => {
                tracing::debug!("CASING MODE enable: {}", mode);
                Ok(mode)
            }
            Err(e) => {
                tracing::error!(
                    "Invalid input for:{}, source type:{}, input value:{}, valid values:[{}]",
                    CASING_MODE_CONFIGURATION_KEY,
                    self.source_type,
                    configured,
                    FederationSdkCasingMode::ALL
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                Err(e)
            }
        }
    }
}
