use std::collections::HashMap;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::{ArrowType, BlockAllocatorImpl, BlockUtils, Value};
use athena_federation_sdk::domain::predicate::expression::{
    ConstantExpression, FederationExpression, FunctionCallExpression, VariableExpression,
    DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
};
use athena_federation_sdk::domain::predicate::functions::FunctionName;
use athena_federation_sdk::domain::predicate::{Constraints, DEFAULT_NO_LIMIT};
use athena_federation_sdk::Result;
use athena_jdbc::manager::{FederationExpressionParser, TypeAndValue};
use indexmap::IndexMap;

const TEST_FUNCTION_NAME: &str = "ADD";
const TEST_IS_NULL_FUNCTION: &str = "IS_NULL";

struct TestParser;

impl FederationExpressionParser for TestParser {
    fn map_function_to_data_source_syntax(
        &self,
        function_name: &FunctionName,
        _arrow_type: &ArrowType,
        arguments: &[String],
    ) -> Result<String> {
        Ok(format!(
            "{}({})",
            function_name.function_name(),
            arguments.join(",")
        ))
    }
}

fn int32() -> ArrowType {
    DataType::Int32
}

fn var(name: &str, arrow_type: ArrowType) -> FederationExpression {
    FederationExpression::Variable(VariableExpression::new(name, arrow_type).unwrap())
}

fn constant<V: Into<Value> + Clone>(
    rows: usize,
    value: V,
    arrow_type: ArrowType,
) -> ConstantExpression {
    let allocator = BlockAllocatorImpl::new();
    let block = BlockUtils::new_block(
        &allocator,
        DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
        &arrow_type,
        vec![value; rows],
    )
    .unwrap();
    ConstantExpression::new(Some(block), arrow_type)
}

fn call(
    arrow_type: ArrowType,
    name: &str,
    args: Vec<FederationExpression>,
) -> FunctionCallExpression {
    FunctionCallExpression::new(arrow_type, FunctionName::new(name), args)
}

fn constraints_with(expressions: Vec<FederationExpression>) -> Constraints {
    Constraints::new(
        IndexMap::new(),
        expressions,
        Vec::new(),
        DEFAULT_NO_LIMIT,
        HashMap::new(),
        None,
    )
}

fn parse(expressions: Vec<FunctionCallExpression>) -> Vec<String> {
    let constraints = constraints_with(
        expressions
            .into_iter()
            .map(FederationExpression::FunctionCall)
            .collect(),
    );
    let mut acc: Vec<TypeAndValue> = Vec::new();
    TestParser
        .parse_complex_expressions(&[], &constraints, &mut acc)
        .unwrap()
}

#[test]
fn test_parse_variable_expression() {
    let expr = VariableExpression::new("column", int32()).unwrap();
    assert_eq!("column", TestParser.parse_variable_expression(&expr));
}

#[test]
fn test_parse_constant_expression() {
    let expr = constant(2, "val", DataType::Utf8);
    let mut acc = Vec::new();
    let result = TestParser
        .parse_constant_expression(&expr, &mut acc)
        .unwrap();
    assert_eq!("?,?", result);
    assert_eq!(2, acc.len());
    assert_eq!(&Value::String("val".to_string()), acc[0].value());
}

#[test]
fn test_parse_function_call_expression_with_all_types() {
    let nested = call(
        int32(),
        TEST_FUNCTION_NAME,
        vec![
            var("age", int32()),
            FederationExpression::Constant(constant(1, 25, int32())),
        ],
    );
    let mut acc = Vec::new();
    let result = TestParser
        .parse_function_call_expression(&nested, &mut acc)
        .unwrap();
    assert!(result.starts_with("ADD("));
    assert_eq!(1, acc.len());
    assert_eq!(&Value::Int(25), acc[0].value());
}

#[test]
fn test_parse_function_call_expression_with_unknown_argument() {
    let function_call = call(
        DataType::Boolean,
        "EQUALS",
        vec![
            var("a", int32()),
            FederationExpression::Constant(constant(1, 1, int32())),
            FederationExpression::FunctionCall(call(int32(), "NESTED", vec![var("b", int32())])),
        ],
    );
    let mut acc = Vec::new();
    let result = TestParser
        .parse_function_call_expression(&function_call, &mut acc)
        .unwrap();
    assert_eq!("EQUALS(a,?,NESTED(b))", result);
    let constraints = constraints_with(vec![var("a", int32())]);
    let err = TestParser
        .parse_complex_expressions(&[], &constraints, &mut acc)
        .unwrap_err();
    assert!(err.to_string().contains("cannot be cast to class"));
}

#[test]
fn test_parse_complex_expressions_with_empty_constraints() {
    let mut acc = Vec::new();
    assert!(TestParser
        .parse_complex_expressions(&[], &Constraints::empty(), &mut acc)
        .unwrap()
        .is_empty());
    assert!(TestParser
        .parse_complex_expressions(&[], &constraints_with(Vec::new()), &mut acc)
        .unwrap()
        .is_empty());
}

#[test]
fn test_parse_complex_expressions_single_function() {
    let expressions = parse(vec![call(
        DataType::Boolean,
        TEST_IS_NULL_FUNCTION,
        vec![var("col", int32())],
    )]);
    assert_eq!(vec!["IS_NULL(col)".to_string()], expressions);
}

#[test]
fn test_parse_complex_expressions_multiple_functions() {
    let expressions = parse(vec![
        call(
            DataType::Boolean,
            TEST_IS_NULL_FUNCTION,
            vec![var("col1", int32())],
        ),
        call(
            DataType::Boolean,
            "NOT_NULL",
            vec![var("col2", DataType::Utf8)],
        ),
    ]);
    assert_eq!(
        vec!["IS_NULL(col1)".to_string(), "NOT_NULL(col2)".to_string()],
        expressions
    );
}

#[test]
fn test_parse_constant_expression_single_value() {
    let expr = constant(1, 42, int32());
    let mut acc = Vec::new();
    assert_eq!(
        "?",
        TestParser
            .parse_constant_expression(&expr, &mut acc)
            .unwrap()
    );
    assert_eq!(1, acc.len());
    assert_eq!(&Value::Int(42), acc[0].value());
}

#[test]
fn test_parse_variable_expression_different_types() {
    for (name, arrow_type) in [
        ("int_col", int32()),
        ("string_col", DataType::Utf8),
        ("bool_col", DataType::Boolean),
    ] {
        let expr = VariableExpression::new(name, arrow_type).unwrap();
        assert_eq!(name, TestParser.parse_variable_expression(&expr));
    }
}

#[test]
fn test_parse_complex_expressions_with_nested_functions() {
    let subtract = call(
        int32(),
        "SUBTRACT",
        vec![var("col1", int32()), var("col2", int32())],
    );
    let multiply = call(
        int32(),
        "MULTIPLY",
        vec![var("col1", int32()), var("col2", int32())],
    );
    let add = call(
        int32(),
        TEST_FUNCTION_NAME,
        vec![
            FederationExpression::FunctionCall(subtract),
            FederationExpression::FunctionCall(multiply),
        ],
    );
    let expressions = parse(vec![add]);
    assert_eq!(1, expressions.len());
    assert!(expressions[0].contains("ADD"));
    assert!(expressions[0].contains("SUBTRACT"));
    assert!(expressions[0].contains("MULTIPLY"));
}

#[test]
fn test_parse_complex_expressions_with_case_when() {
    let greater = call(
        DataType::Boolean,
        "GREATER_THAN",
        vec![
            var("age", int32()),
            FederationExpression::Constant(constant(1, 18, int32())),
        ],
    );
    let case = call(
        int32(),
        "CASE_WHEN",
        vec![
            FederationExpression::FunctionCall(greater),
            var("salary", int32()),
            var("bonus", int32()),
        ],
    );
    let expressions = parse(vec![case]);
    assert_eq!(1, expressions.len());
    assert!(expressions[0].contains("CASE_WHEN"));
    assert!(expressions[0].contains("GREATER_THAN"));
}

#[test]
fn test_parse_complex_expressions_with_aggregate_functions() {
    let add = call(
        int32(),
        TEST_FUNCTION_NAME,
        vec![var("col1", int32()), var("col1", int32())],
    );
    let sum = call(
        int32(),
        "SUM",
        vec![FederationExpression::FunctionCall(add)],
    );
    let expressions = parse(vec![sum]);
    assert_eq!(1, expressions.len());
    assert!(expressions[0].contains("SUM"));
    assert!(expressions[0].contains("ADD"));
}

#[test]
fn test_parse_complex_expressions_with_string_functions() {
    let like = call(
        DataType::Boolean,
        "LIKE",
        vec![var("name", DataType::Utf8), var("pattern", DataType::Utf8)],
    );
    let concat = call(
        DataType::Utf8,
        "CONCAT",
        vec![
            var("name", DataType::Utf8),
            FederationExpression::Constant(constant(1, "_suffix", DataType::Utf8)),
        ],
    );
    let expressions = parse(vec![like, concat]);
    assert_eq!(2, expressions.len());
    assert!(expressions[0].contains("LIKE"));
    assert!(expressions[1].contains("CONCAT"));
}

#[test]
fn test_parse_complex_expressions_with_null_handling() {
    let coalesce = call(
        int32(),
        "COALESCE",
        vec![
            var("col1", int32()),
            var("col2", int32()),
            FederationExpression::Constant(constant(1, 0, int32())),
        ],
    );
    let is_null = call(
        DataType::Boolean,
        TEST_IS_NULL_FUNCTION,
        vec![var("col1", int32())],
    );
    let expressions = parse(vec![coalesce, is_null]);
    assert_eq!(2, expressions.len());
    assert!(expressions[0].contains("COALESCE"));
    assert!(expressions[1].contains("IS_NULL"));
}
