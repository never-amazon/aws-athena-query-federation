use std::fmt;

use crate::data::ArrowType;

use super::constant_expression::ConstantExpression;
use super::function_call_expression::FunctionCallExpression;
use super::variable_expression::VariableExpression;

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
