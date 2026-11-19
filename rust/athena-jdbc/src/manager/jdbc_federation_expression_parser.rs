use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::expression::VariableExpression;
use athena_federation_sdk::domain::predicate::functions::{
    FunctionName, OperatorType, StandardFunctions,
};
use athena_federation_sdk::{FederationError, Result};

use super::federation_expression_parser::FederationExpressionParser;
use crate::validate::null_pointer;

const NULL_FUNCTION_ENUM: &str = "Cannot invoke \"com.amazonaws.athena.connector.lambda.domain.predicate.functions.StandardFunctions.getOperatorType()\" because \"functionEnum\" is null";

pub trait JdbcFederationExpressionParser: FederationExpressionParser {
    fn quote_char(&self) -> &str;

    fn write_array_constructor_clause(
        &self,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String>;
}

pub trait JdbcFederationExpressionParserExt: JdbcFederationExpressionParser {
    fn jdbc_parse_variable_expression(&self, variable_expression: &VariableExpression) -> String;

    fn jdbc_map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String>;
}

impl<T: JdbcFederationExpressionParser + ?Sized> JdbcFederationExpressionParserExt for T {
    fn jdbc_parse_variable_expression(&self, variable_expression: &VariableExpression) -> String {
        let quote = self.quote_char();
        format!("{quote}{}{quote}", variable_expression.column_name())
    }

    fn jdbc_map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        let function_enum = StandardFunctions::from_function_name(function_name)
            .ok_or_else(|| null_pointer(NULL_FUNCTION_ENUM))?;
        let operator_type = function_enum.operator_type();

        if arguments.is_empty() {
            return Err(FederationError::invalid_input(
                "Arguments cannot be null or empty.",
            ));
        }
        match operator_type {
            OperatorType::UNARY if arguments.len() != 1 => {
                return Err(FederationError::invalid_input(format!(
                    "Unary function type {} was provided with {} arguments.",
                    function_name.function_name(),
                    arguments.len()
                )));
            }
            OperatorType::BINARY if arguments.len() != 2 => {
                return Err(FederationError::invalid_input(format!(
                    "Binary function type {} was provided with {} arguments.",
                    function_name.function_name(),
                    arguments.len()
                )));
            }
            _ => {}
        }

        let clause = match function_enum {
            StandardFunctions::ADD_FUNCTION_NAME => arguments.join(" + "),
            StandardFunctions::AND_FUNCTION_NAME => arguments.join(" AND "),
            StandardFunctions::ARRAY_CONSTRUCTOR_FUNCTION_NAME => {
                self.write_array_constructor_clause(arrow_type, arguments)?
            }
            StandardFunctions::DIVIDE_FUNCTION_NAME => arguments.join(" / "),
            StandardFunctions::EQUAL_OPERATOR_FUNCTION_NAME => arguments.join(" = "),
            StandardFunctions::GREATER_THAN_OPERATOR_FUNCTION_NAME => arguments.join(" > "),
            StandardFunctions::GREATER_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME => {
                arguments.join(" >= ")
            }
            StandardFunctions::IN_PREDICATE_FUNCTION_NAME => {
                format!("{} IN {}", arguments[0], arguments[1])
            }
            StandardFunctions::IS_DISTINCT_FROM_OPERATOR_FUNCTION_NAME => {
                format!("{} IS DISTINCT FROM {}", arguments[0], arguments[1])
            }
            StandardFunctions::IS_NULL_FUNCTION_NAME => format!("{} IS NULL", arguments[0]),
            StandardFunctions::LESS_THAN_OPERATOR_FUNCTION_NAME => arguments.join(" < "),
            StandardFunctions::LESS_THAN_OR_EQUAL_OPERATOR_FUNCTION_NAME => arguments.join(" <= "),
            StandardFunctions::LIKE_PATTERN_FUNCTION_NAME => {
                format!("{} LIKE {}", arguments[0], arguments[1])
            }
            StandardFunctions::MODULUS_FUNCTION_NAME => arguments.join(" % "),
            StandardFunctions::MULTIPLY_FUNCTION_NAME => arguments.join(" * "),
            StandardFunctions::NEGATE_FUNCTION_NAME => format!("-{}", arguments[0]),
            StandardFunctions::NOT_EQUAL_OPERATOR_FUNCTION_NAME => arguments.join(" <> "),
            StandardFunctions::NOT_FUNCTION_NAME => format!(" NOT {}", arguments[0]),
            StandardFunctions::NULLIF_FUNCTION_NAME => {
                format!("NULLIF({}, {})", arguments[0], arguments[1])
            }
            StandardFunctions::OR_FUNCTION_NAME => arguments.join(" OR "),
            StandardFunctions::SUBTRACT_FUNCTION_NAME => arguments.join(" - "),
        };
        Ok(format!("({clause})"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardFederationExpressionParser {
    quote_char: String,
}

impl StandardFederationExpressionParser {
    pub fn new(quote_char: impl Into<String>) -> StandardFederationExpressionParser {
        StandardFederationExpressionParser {
            quote_char: quote_char.into(),
        }
    }
}

impl FederationExpressionParser for StandardFederationExpressionParser {
    fn map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        self.jdbc_map_function_to_data_source_syntax(function_name, arrow_type, arguments)
    }

    fn parse_variable_expression(&self, variable_expression: &VariableExpression) -> String {
        self.jdbc_parse_variable_expression(variable_expression)
    }
}

impl JdbcFederationExpressionParser for StandardFederationExpressionParser {
    fn quote_char(&self) -> &str {
        &self.quote_char
    }

    fn write_array_constructor_clause(
        &self,
        _arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        Ok(arguments.join(", "))
    }
}
