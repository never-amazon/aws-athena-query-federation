use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::exceptions::{FederationError, Result};

pub const CASING_MODE_CONFIGURATION_KEY: &str = "casing_mode";
pub const DEFAULT_GLUE_CONNECTION: &str = "glue_connection";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FederationSdkCasingMode {
    None,
    Lower,
    Upper,
    CaseInsensitiveSearch,
    Annotation,
}

impl FederationSdkCasingMode {
    pub fn values() -> &'static [FederationSdkCasingMode] {
        use FederationSdkCasingMode::{Annotation, CaseInsensitiveSearch, Lower, None, Upper};
        &[None, Lower, Upper, CaseInsensitiveSearch, Annotation]
    }

    pub fn name(&self) -> &'static str {
        match self {
            FederationSdkCasingMode::None => "NONE",
            FederationSdkCasingMode::Lower => "LOWER",
            FederationSdkCasingMode::Upper => "UPPER",
            FederationSdkCasingMode::CaseInsensitiveSearch => "CASE_INSENSITIVE_SEARCH",
            FederationSdkCasingMode::Annotation => "ANNOTATION",
        }
    }

    pub fn value_of(name: &str) -> Result<FederationSdkCasingMode> {
        FederationSdkCasingMode::values()
            .iter()
            .copied()
            .find(|m| m.name() == name)
            .ok_or_else(|| {
                FederationError::IllegalArgument(format!(
                    "No enum constant com.amazonaws.athena.connector.lambda.resolver.CaseResolver.FederationSDKCasingMode.{name}"
                ))
            })
    }
}

impl fmt::Display for FederationSdkCasingMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for FederationSdkCasingMode {
    type Err = FederationError;

    fn from_str(s: &str) -> Result<Self> {
        FederationSdkCasingMode::value_of(s)
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
        match FederationSdkCasingMode::value_of(&configured.to_uppercase()) {
            Ok(mode) => {
                tracing::debug!("CASING MODE enable: {}", mode);
                Ok(mode)
            }
            Err(e) => {
                tracing::error!(
                    "Invalid input for:{}, source type:{}, input value:{}, valid values:{:?}",
                    CASING_MODE_CONFIGURATION_KEY,
                    self.source_type,
                    configured,
                    FederationSdkCasingMode::values()
                );
                Err(e)
            }
        }
    }
}
