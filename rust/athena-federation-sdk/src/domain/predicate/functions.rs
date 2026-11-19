use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardFunctions {
    And,
    Or,
    Not,
    IsNull,
    NullIf,
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    IsDistinctFrom,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulus,
    Negate,
    LikePattern,
    In,
    ArrayConstructor,
}

impl StandardFunctions {
    pub const ALL: [StandardFunctions; 21] = [
        StandardFunctions::And,
        StandardFunctions::Or,
        StandardFunctions::Not,
        StandardFunctions::IsNull,
        StandardFunctions::NullIf,
        StandardFunctions::Equal,
        StandardFunctions::NotEqual,
        StandardFunctions::LessThan,
        StandardFunctions::LessThanOrEqual,
        StandardFunctions::GreaterThan,
        StandardFunctions::GreaterThanOrEqual,
        StandardFunctions::IsDistinctFrom,
        StandardFunctions::Add,
        StandardFunctions::Subtract,
        StandardFunctions::Multiply,
        StandardFunctions::Divide,
        StandardFunctions::Modulus,
        StandardFunctions::Negate,
        StandardFunctions::LikePattern,
        StandardFunctions::In,
        StandardFunctions::ArrayConstructor,
    ];

    fn parts(self) -> (&'static str, OperatorType) {
        use OperatorType::{Binary, Unary, Vararg};
        match self {
            StandardFunctions::And => ("$and", Vararg),
            StandardFunctions::Or => ("$or", Vararg),
            StandardFunctions::Not => ("$not", Unary),
            StandardFunctions::IsNull => ("$is_null", Unary),
            StandardFunctions::NullIf => ("$nullif", Binary),
            StandardFunctions::Equal => ("$equal", Binary),
            StandardFunctions::NotEqual => ("$not_equal", Binary),
            StandardFunctions::LessThan => ("$less_than", Binary),
            StandardFunctions::LessThanOrEqual => ("$less_than_or_equal", Binary),
            StandardFunctions::GreaterThan => ("$greater_than", Binary),
            StandardFunctions::GreaterThanOrEqual => ("$greater_than_or_equal", Binary),
            StandardFunctions::IsDistinctFrom => ("$is_distinct_from", Binary),
            StandardFunctions::Add => ("$add", Binary),
            StandardFunctions::Subtract => ("$subtract", Binary),
            StandardFunctions::Multiply => ("$multiply", Binary),
            StandardFunctions::Divide => ("$divide", Binary),
            StandardFunctions::Modulus => ("$modulus", Binary),
            StandardFunctions::Negate => ("$negate", Unary),
            StandardFunctions::LikePattern => ("$like_pattern", Binary),
            StandardFunctions::In => ("$in", Binary),
            StandardFunctions::ArrayConstructor => ("$array", Vararg),
        }
    }

    pub fn function_name(&self) -> FunctionName {
        FunctionName::new(self.parts().0)
    }

    pub fn operator_type(&self) -> OperatorType {
        self.parts().1
    }

    pub fn from_function_name(function_name: &FunctionName) -> Option<StandardFunctions> {
        StandardFunctions::ALL
            .into_iter()
            .find(|f| f.parts().0 == function_name.function_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionName {
    function_name: String,
}

impl FunctionName {
    pub fn new(function_name: impl Into<String>) -> FunctionName {
        FunctionName {
            function_name: function_name.into(),
        }
    }

    pub fn function_name(&self) -> &str {
        &self.function_name
    }
}

impl fmt::Display for FunctionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FunctionName{{functionName={}}}", self.function_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperatorType {
    Unary,
    Binary,
    Vararg,
}
