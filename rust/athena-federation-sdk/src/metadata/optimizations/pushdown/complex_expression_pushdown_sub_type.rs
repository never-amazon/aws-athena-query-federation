use super::SubTypeProperties;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComplexExpressionPushdownSubType {
    SupportedFunctionExpressionTypes,
}

impl ComplexExpressionPushdownSubType {
    pub fn values() -> &'static [ComplexExpressionPushdownSubType] {
        &[ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes]
    }

    pub fn name(&self) -> &'static str {
        "SUPPORTED_FUNCTION_EXPRESSION_TYPES"
    }

    pub fn sub_type(&self) -> &'static str {
        "supported_function_expression_types"
    }

    pub fn with_sub_type_properties(&self, properties: &[&str]) -> Result<SubTypeProperties> {
        if properties.is_empty() {
            return Err(FederationError::invalid_input(
                "Connectors that support function expressions must provide a list of supported functions. See Documentation for more details",
            ));
        }
        Ok(SubTypeProperties::new(
            self.sub_type(),
            properties
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ))
    }
}
