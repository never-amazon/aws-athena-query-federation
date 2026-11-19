use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::ArrowType;
use crate::exceptions::{FederationError, Result};

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
