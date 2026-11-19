use std::fmt;

use crate::data::ArrowType;
use crate::domain::predicate::functions::FunctionName;

use super::federation_expression::FederationExpression;

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCallExpression {
    arrow_type: ArrowType,
    function_name: FunctionName,
    arguments: Vec<FederationExpression>,
}

impl FunctionCallExpression {
    pub fn new(
        arrow_type: ArrowType,
        function_name: FunctionName,
        arguments: Vec<FederationExpression>,
    ) -> FunctionCallExpression {
        FunctionCallExpression {
            arrow_type,
            function_name,
            arguments,
        }
    }

    pub fn function_name(&self) -> &FunctionName {
        &self.function_name
    }

    pub fn arguments(&self) -> &[FederationExpression] {
        &self.arguments
    }

    pub fn children(&self) -> &[FederationExpression] {
        &self.arguments
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }
}

impl fmt::Display for FunctionCallExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args: Vec<String> = self
            .arguments
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        write!(
            f,
            "FunctionCallExpression[functionName={}, arguments=[{}]]",
            self.function_name,
            args.join(", ")
        )
    }
}
