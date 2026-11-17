use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubstraitOperator {
    Equal,
    NotEqual,
    GreaterThan,
    LessThan,
    GreaterThanOrEqualTo,
    LessThanOrEqualTo,
    IsNull,
    IsNotNull,
    And,
    Or,
    Not,
    NotIn,
    Nor,
    Nand,
}

impl SubstraitOperator {
    pub fn get_symbol(&self) -> &'static str {
        match self {
            SubstraitOperator::Equal => "=",
            SubstraitOperator::NotEqual => "!=",
            SubstraitOperator::GreaterThan => ">",
            SubstraitOperator::LessThan => "<",
            SubstraitOperator::GreaterThanOrEqualTo => ">=",
            SubstraitOperator::LessThanOrEqualTo => "<=",
            SubstraitOperator::IsNull => "IS NULL",
            SubstraitOperator::IsNotNull => "IS NOT NULL",
            SubstraitOperator::And => "AND",
            SubstraitOperator::Or => "OR",
            SubstraitOperator::Not => "NOT",
            SubstraitOperator::NotIn => "NOT IN",
            SubstraitOperator::Nor => "NOR",
            SubstraitOperator::Nand => "NAND",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            SubstraitOperator::Equal => "EQUAL",
            SubstraitOperator::NotEqual => "NOT_EQUAL",
            SubstraitOperator::GreaterThan => "GREATER_THAN",
            SubstraitOperator::LessThan => "LESS_THAN",
            SubstraitOperator::GreaterThanOrEqualTo => "GREATER_THAN_OR_EQUAL_TO",
            SubstraitOperator::LessThanOrEqualTo => "LESS_THAN_OR_EQUAL_TO",
            SubstraitOperator::IsNull => "IS_NULL",
            SubstraitOperator::IsNotNull => "IS_NOT_NULL",
            SubstraitOperator::And => "AND",
            SubstraitOperator::Or => "OR",
            SubstraitOperator::Not => "NOT",
            SubstraitOperator::NotIn => "NOT_IN",
            SubstraitOperator::Nor => "NOR",
            SubstraitOperator::Nand => "NAND",
        }
    }
}

impl fmt::Display for SubstraitOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
