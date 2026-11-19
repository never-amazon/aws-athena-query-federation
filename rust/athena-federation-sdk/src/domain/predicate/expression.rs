use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, Block};
use crate::domain::predicate::FunctionName;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum FederationExpression {
    FunctionCall(FunctionCallExpression),
    Variable(VariableExpression),
    Constant(ConstantExpression),
}

impl FederationExpression {
    pub fn type_name(&self) -> &'static str {
        match self {
            FederationExpression::FunctionCall(_) => "functionCall",
            FederationExpression::Variable(_) => "variable",
            FederationExpression::Constant(_) => "constant",
        }
    }

    pub fn get_type(&self) -> &ArrowType {
        match self {
            FederationExpression::FunctionCall(e) => e.get_type(),
            FederationExpression::Variable(e) => e.get_type(),
            FederationExpression::Constant(e) => e.get_type(),
        }
    }

    pub fn children(&self) -> &[FederationExpression] {
        match self {
            FederationExpression::FunctionCall(e) => e.children(),
            FederationExpression::Variable(_) | FederationExpression::Constant(_) => &[],
        }
    }
}

impl From<FunctionCallExpression> for FederationExpression {
    fn from(e: FunctionCallExpression) -> Self {
        FederationExpression::FunctionCall(e)
    }
}

impl From<VariableExpression> for FederationExpression {
    fn from(e: VariableExpression) -> Self {
        FederationExpression::Variable(e)
    }
}

impl From<ConstantExpression> for FederationExpression {
    fn from(e: ConstantExpression) -> Self {
        FederationExpression::Constant(e)
    }
}

impl fmt::Display for FederationExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FederationExpression::FunctionCall(e) => write!(f, "{e}"),
            FederationExpression::Variable(e) => write!(f, "{e}"),
            FederationExpression::Constant(e) => write!(f, "{e}"),
        }
    }
}

pub const DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME: &str = "col1";

#[derive(Debug, Clone, PartialEq)]
pub struct ConstantExpression {
    value_block: Option<Block>,
    arrow_type: ArrowType,
}

impl ConstantExpression {
    pub fn new(value_block: Option<Block>, arrow_type: ArrowType) -> ConstantExpression {
        ConstantExpression {
            value_block,
            arrow_type,
        }
    }

    pub fn values(&self) -> Option<&Block> {
        self.value_block.as_ref()
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn children(&self) -> &[super::FederationExpression] {
        &[]
    }
}

impl fmt::Display for ConstantExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.value_block {
            Some(b) => write!(f, "{}::{}", b, arrow_type_to_string(&self.arrow_type)),
            None => write!(f, "null::{}", arrow_type_to_string(&self.arrow_type)),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct VariableExpression {
    column_name: String,
    arrow_type: ArrowType,
}

impl VariableExpression {
    pub fn new(
        column_name: impl Into<String>,
        arrow_type: ArrowType,
    ) -> Result<VariableExpression> {
        let column_name = column_name.into();
        if column_name.is_empty() {
            return Err(FederationError::invalid_input("columnName is empty"));
        }
        Ok(VariableExpression {
            column_name,
            arrow_type,
        })
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn children(&self) -> &[super::FederationExpression] {
        &[]
    }
}

impl fmt::Display for VariableExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}",
            self.column_name,
            arrow_type_to_string(&self.arrow_type)
        )
    }
}
