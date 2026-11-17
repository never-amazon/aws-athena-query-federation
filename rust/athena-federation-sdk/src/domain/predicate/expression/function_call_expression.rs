use std::fmt;

use crate::data::java_format::java_string_hash;
use crate::data::ArrowType;
use crate::domain::predicate::functions::FunctionName;
use crate::domain::predicate::marker::{objects_hash, type_hash};

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

    pub fn hash_code(&self) -> i32 {
        let args = self.arguments.iter().fold(1i32, |acc, a| {
            acc.wrapping_mul(31).wrapping_add(a.hash_code())
        });
        objects_hash(&[
            java_string_hash(self.function_name.function_name()),
            args,
            type_hash(&self.arrow_type),
        ])
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
