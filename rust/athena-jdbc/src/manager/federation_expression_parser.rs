use arrow::datatypes::FieldRef;

use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::domain::predicate::expression::{
    ConstantExpression, FederationExpression, FunctionCallExpression, VariableExpression,
    DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
};
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::domain::predicate::Constraints;
use athena_federation_sdk::{FederationError, Result};

use super::type_and_value::TypeAndValue;
use crate::validate::null_pointer;

const FUNCTION_CALL_EXPRESSION_CLASS: &str =
    "com.amazonaws.athena.connector.lambda.domain.predicate.expression.FunctionCallExpression";
const EXPRESSION_PACKAGE: &str =
    "com.amazonaws.athena.connector.lambda.domain.predicate.expression";

fn class_cast(simple_name: &str) -> FederationError {
    FederationError::ClassCast(format!(
        "class {EXPRESSION_PACKAGE}.{simple_name} cannot be cast to class {FUNCTION_CALL_EXPRESSION_CLASS}"
    ))
}

pub trait FederationExpressionParser: Send + Sync {
    fn map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String>;

    fn parse_complex_expressions(
        &self,
        _columns: &[FieldRef],
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

    fn parse_function_call_expression(
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
            function_call_expression.get_type(),
            &arguments,
        )
    }

    fn parse_constant_expression(
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
                constant_expression.get_type().clone(),
                vector.get_object(i),
            )?);
        }
        Ok(vec!["?"; row_count].join(","))
    }

    fn parse_variable_expression(&self, variable_expression: &VariableExpression) -> String {
        variable_expression.column_name().to_string()
    }
}
