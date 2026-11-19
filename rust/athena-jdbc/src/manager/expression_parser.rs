use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::{
    ConstantExpression, Constraints, FederationExpression, FunctionCallExpression, FunctionName,
    OperatorType, StandardFunctions, VariableExpression, DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
};
use athena_federation_sdk::metadata::{
    Capability, ComplexExpressionPushdownSubType, FilterPushdownSubType,
};
use athena_federation_sdk::{FederationError, Result};

use super::type_and_value::TypeAndValue;
use crate::validate::null_pointer;

const FUNCTION_CALL_EXPRESSION_CLASS: &str =
    "com.amazonaws.athena.connector.lambda.domain.predicate.expression.FunctionCallExpression";
const EXPRESSION_PACKAGE: &str =
    "com.amazonaws.athena.connector.lambda.domain.predicate.expression";
const NULL_FUNCTION_ENUM: &str = "Cannot invoke \"com.amazonaws.athena.connector.lambda.domain.predicate.functions.StandardFunctions.getOperatorType()\" because \"functionEnum\" is null";

fn class_cast(simple_name: &str) -> FederationError {
    FederationError::ClassCast(format!(
        "class {EXPRESSION_PACKAGE}.{simple_name} cannot be cast to class {FUNCTION_CALL_EXPRESSION_CLASS}"
    ))
}

pub fn filter_capability() -> Capability {
    Capability::Filter(vec![
        FilterPushdownSubType::SortedRangeSet,
        FilterPushdownSubType::NullableComparison,
    ])
}

pub fn complex_expression_capability() -> Result<Capability> {
    let function_names: Vec<String> = StandardFunctions::ALL
        .iter()
        .map(|f| f.function_name().function_name().to_string())
        .collect();
    let function_names: Vec<&str> = function_names.iter().map(String::as_str).collect();
    Ok(Capability::ComplexExpression(vec![
        ComplexExpressionPushdownSubType::SupportedFunctionExpressionTypes
            .with_sub_type_properties(&function_names)?,
    ]))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionParser {
    quote_char: String,
}

impl ExpressionParser {
    pub fn new(quote_char: impl Into<String>) -> ExpressionParser {
        ExpressionParser {
            quote_char: quote_char.into(),
        }
    }

    pub fn parse_complex_expressions(
        &self,
        constraints: &Constraints,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<Vec<String>> {
        constraints
            .expression()
            .iter()
            .map(|expression| match expression {
                FederationExpression::FunctionCall(call) => {
                    self.parse_function_call_expression(call, accumulator)
                }
                FederationExpression::Variable(_) => Err(class_cast("VariableExpression")),
                FederationExpression::Constant(_) => Err(class_cast("ConstantExpression")),
            })
            .collect()
    }

    pub fn parse_function_call_expression(
        &self,
        function_call_expression: &FunctionCallExpression,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let mut arguments = Vec::with_capacity(function_call_expression.arguments().len());
        for argument in function_call_expression.arguments() {
            let parsed = match argument {
                FederationExpression::Constant(constant) => {
                    self.parse_constant_expression(constant, accumulator)?
                }
                FederationExpression::Variable(variable) => {
                    self.parse_variable_expression(variable)
                }
                FederationExpression::FunctionCall(call) => {
                    self.parse_function_call_expression(call, accumulator)?
                }
            };
            arguments.push(parsed);
        }
        self.map_function_to_data_source_syntax(
            function_call_expression.function_name(),
            &arguments,
        )
    }

    pub fn parse_constant_expression(
        &self,
        constant_expression: &ConstantExpression,
        accumulator: &mut Vec<TypeAndValue>,
    ) -> Result<String> {
        let values = constant_expression.values().ok_or_else(|| {
            null_pointer("Cannot invoke \"com.amazonaws.athena.connector.lambda.data.Block.getFieldReader(String)\" because \"values\" is null")
        })?;
        let vector = values
            .get_field_vector(DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME)
            .ok_or_else(|| {
                null_pointer(format!(
                    "Cannot invoke \"org.apache.arrow.vector.FieldVector.getReader()\" because the block has no field {DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME}"
                ))
            })?;
        let row_count = values.row_count();
        for i in 0..row_count {
            accumulator.push(TypeAndValue::new(
                ArrowType::clone(constant_expression.get_type()),
                vector.get_object(i),
            )?);
        }
        Ok(vec!["?"; row_count].join(","))
    }

    pub fn parse_variable_expression(&self, variable_expression: &VariableExpression) -> String {
        let quote = &self.quote_char;
        format!("{quote}{}{quote}", variable_expression.column_name())
    }

    pub fn write_array_constructor_clause(&self, arguments: &[String]) -> String {
        arguments.join(", ")
    }

    pub fn map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
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
            OperatorType::Unary if arguments.len() != 1 => {
                return Err(FederationError::invalid_input(format!(
                    "Unary function type {} was provided with {} arguments.",
                    function_name.function_name(),
                    arguments.len()
                )));
            }
            OperatorType::Binary if arguments.len() != 2 => {
                return Err(FederationError::invalid_input(format!(
                    "Binary function type {} was provided with {} arguments.",
                    function_name.function_name(),
                    arguments.len()
                )));
            }
            _ => {}
        }

        let clause = match function_enum {
            StandardFunctions::Add => arguments.join(" + "),
            StandardFunctions::And => arguments.join(" AND "),
            StandardFunctions::ArrayConstructor => self.write_array_constructor_clause(arguments),
            StandardFunctions::Divide => arguments.join(" / "),
            StandardFunctions::Equal => arguments.join(" = "),
            StandardFunctions::GreaterThan => arguments.join(" > "),
            StandardFunctions::GreaterThanOrEqual => arguments.join(" >= "),
            StandardFunctions::In => {
                format!("{} IN {}", arguments[0], arguments[1])
            }
            StandardFunctions::IsDistinctFrom => {
                format!("{} IS DISTINCT FROM {}", arguments[0], arguments[1])
            }
            StandardFunctions::IsNull => format!("{} IS NULL", arguments[0]),
            StandardFunctions::LessThan => arguments.join(" < "),
            StandardFunctions::LessThanOrEqual => arguments.join(" <= "),
            StandardFunctions::LikePattern => {
                format!("{} LIKE {}", arguments[0], arguments[1])
            }
            StandardFunctions::Modulus => arguments.join(" % "),
            StandardFunctions::Multiply => arguments.join(" * "),
            StandardFunctions::Negate => format!("-{}", arguments[0]),
            StandardFunctions::NotEqual => arguments.join(" <> "),
            StandardFunctions::Not => format!(" NOT {}", arguments[0]),
            StandardFunctions::NullIf => {
                format!("NULLIF({}, {})", arguments[0], arguments[1])
            }
            StandardFunctions::Or => arguments.join(" OR "),
            StandardFunctions::Subtract => arguments.join(" - "),
        };
        Ok(format!("({clause})"))
    }
}
