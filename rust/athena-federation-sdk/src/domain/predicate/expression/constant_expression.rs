use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, Block};

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
