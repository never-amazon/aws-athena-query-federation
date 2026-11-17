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

    pub fn with_sub_type_properties(
        &self,
        properties: &[&str],
    ) -> Result<ComplexExpressionSubTypeProperties> {
        if properties.is_empty() {
            return Err(FederationError::invalid_input(
                "Connectors that support function expressions must provide a list of supported functions. See Documentation for more details",
            ));
        }
        Ok(ComplexExpressionSubTypeProperties::new(
            self.sub_type(),
            properties
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComplexExpressionSubTypeProperties {
    property_name: String,
    property_values: Vec<String>,
}

impl ComplexExpressionSubTypeProperties {
    pub fn new(
        property_name: impl Into<String>,
        property_values: Vec<String>,
    ) -> ComplexExpressionSubTypeProperties {
        ComplexExpressionSubTypeProperties {
            property_name: property_name.into(),
            property_values,
        }
    }

    pub fn sub_type(&self) -> &str {
        &self.property_name
    }

    pub fn properties(&self) -> &[String] {
        &self.property_values
    }
}
