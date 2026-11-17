use super::function_name::FunctionName;
use super::operator_type::OperatorType;

#[allow(
    non_camel_case_types,
    reason = "variant names mirror the Java enum constants"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardFunctions {
    AND_FUNCTION_NAME,
    OR_FUNCTION_NAME,
    NOT_FUNCTION_NAME,
    IS_NULL_FUNCTION_NAME,
    NULLIF_FUNCTION_NAME,
    EQUAL_OPERATOR_FUNCTION_NAME,
    NOT_EQUAL_OPERATOR_FUNCTION_NAME,
    LESS_THAN_OPERATOR_FUNCTION_NAME,
    LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
    GREATER_THAN_OPERATOR_FUNCTION_NAME,
    GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
    IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME,
    ADD_FUNCTION_NAME,
    SUBTRACT_FUNCTION_NAME,
    MULTIPLY_FUNCTION_NAME,
    DIVIDE_FUNCTION_NAME,
    MODULUS_FUNCTION_NAME,
    NEGATE_FUNCTION_NAME,
    LIKE_PATTERN_FUNCTION_NAME,
    IN_PREDICATE_FUNCTION_NAME,
    ARRAY_CONSTRUCTOR_FUNCTION_NAME,
}

impl StandardFunctions {
    pub fn values() -> &'static [StandardFunctions] {
        use StandardFunctions::{
            ADD_FUNCTION_NAME, AND_FUNCTION_NAME, ARRAY_CONSTRUCTOR_FUNCTION_NAME,
            DIVIDE_FUNCTION_NAME, EQUAL_OPERATOR_FUNCTION_NAME,
            GREATER_THAN_OPERATOR_FUNCTION_NAME, GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
            IN_PREDICATE_FUNCTION_NAME, IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME,
            IS_NULL_FUNCTION_NAME, LESS_THAN_OPERATOR_FUNCTION_NAME,
            LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME, LIKE_PATTERN_FUNCTION_NAME,
            MODULUS_FUNCTION_NAME, MULTIPLY_FUNCTION_NAME, NEGATE_FUNCTION_NAME,
            NOT_EQUAL_OPERATOR_FUNCTION_NAME, NOT_FUNCTION_NAME, NULLIF_FUNCTION_NAME,
            OR_FUNCTION_NAME, SUBTRACT_FUNCTION_NAME,
        };
        &[
            AND_FUNCTION_NAME,
            OR_FUNCTION_NAME,
            NOT_FUNCTION_NAME,
            IS_NULL_FUNCTION_NAME,
            NULLIF_FUNCTION_NAME,
            EQUAL_OPERATOR_FUNCTION_NAME,
            NOT_EQUAL_OPERATOR_FUNCTION_NAME,
            LESS_THAN_OPERATOR_FUNCTION_NAME,
            LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
            GREATER_THAN_OPERATOR_FUNCTION_NAME,
            GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
            IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME,
            ADD_FUNCTION_NAME,
            SUBTRACT_FUNCTION_NAME,
            MULTIPLY_FUNCTION_NAME,
            DIVIDE_FUNCTION_NAME,
            MODULUS_FUNCTION_NAME,
            NEGATE_FUNCTION_NAME,
            LIKE_PATTERN_FUNCTION_NAME,
            IN_PREDICATE_FUNCTION_NAME,
            ARRAY_CONSTRUCTOR_FUNCTION_NAME,
        ]
    }

    fn parts(self) -> (&'static str, OperatorType) {
        use OperatorType::{BINARY, UNARY, VARARG};
        use StandardFunctions::{
            ADD_FUNCTION_NAME, AND_FUNCTION_NAME, ARRAY_CONSTRUCTOR_FUNCTION_NAME,
            DIVIDE_FUNCTION_NAME, EQUAL_OPERATOR_FUNCTION_NAME,
            GREATER_THAN_OPERATOR_FUNCTION_NAME, GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME,
            IN_PREDICATE_FUNCTION_NAME, IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME,
            IS_NULL_FUNCTION_NAME, LESS_THAN_OPERATOR_FUNCTION_NAME,
            LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME, LIKE_PATTERN_FUNCTION_NAME,
            MODULUS_FUNCTION_NAME, MULTIPLY_FUNCTION_NAME, NEGATE_FUNCTION_NAME,
            NOT_EQUAL_OPERATOR_FUNCTION_NAME, NOT_FUNCTION_NAME, NULLIF_FUNCTION_NAME,
            OR_FUNCTION_NAME, SUBTRACT_FUNCTION_NAME,
        };
        match self {
            AND_FUNCTION_NAME => ("$and", VARARG),
            OR_FUNCTION_NAME => ("$or", VARARG),
            NOT_FUNCTION_NAME => ("$not", UNARY),
            IS_NULL_FUNCTION_NAME => ("$is_null", UNARY),
            NULLIF_FUNCTION_NAME => ("$nullif", BINARY),
            EQUAL_OPERATOR_FUNCTION_NAME => ("$equal", BINARY),
            NOT_EQUAL_OPERATOR_FUNCTION_NAME => ("$not_equal", BINARY),
            LESS_THAN_OPERATOR_FUNCTION_NAME => ("$less_than", BINARY),
            LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME => ("$less_than_or_equal", BINARY),
            GREATER_THAN_OPERATOR_FUNCTION_NAME => ("$greater_than", BINARY),
            GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME => ("$greater_than_or_equal", BINARY),
            IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME => ("$is_distinct_from", BINARY),
            ADD_FUNCTION_NAME => ("$add", BINARY),
            SUBTRACT_FUNCTION_NAME => ("$subtract", BINARY),
            MULTIPLY_FUNCTION_NAME => ("$multiply", BINARY),
            DIVIDE_FUNCTION_NAME => ("$divide", BINARY),
            MODULUS_FUNCTION_NAME => ("$modulus", BINARY),
            NEGATE_FUNCTION_NAME => ("$negate", UNARY),
            LIKE_PATTERN_FUNCTION_NAME => ("$like_pattern", BINARY),
            IN_PREDICATE_FUNCTION_NAME => ("$in", BINARY),
            ARRAY_CONSTRUCTOR_FUNCTION_NAME => ("$array", VARARG),
        }
    }

    pub fn function_name(&self) -> FunctionName {
        FunctionName::new(self.parts().0)
    }

    pub fn operator_type(&self) -> OperatorType {
        self.parts().1
    }

    pub fn from_function_name(function_name: &FunctionName) -> Option<StandardFunctions> {
        StandardFunctions::values()
            .iter()
            .copied()
            .find(|f| f.parts().0 == function_name.function_name())
    }
}
