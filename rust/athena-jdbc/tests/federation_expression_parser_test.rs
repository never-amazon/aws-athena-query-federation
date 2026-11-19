use std::collections::HashMap;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::{block_utils, ArrowType, BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{
    ConstantExpression, Constraints, FederationExpression, FunctionCallExpression,
    StandardFunctions, VariableExpression, DEFAULT_CONSTANT_EXPRESSION_BLOCK_NAME,
    DEFAULT_NO_LIMIT,
};
use athena_jdbc::query::{ExpressionParser, TypeAndValue};
use indexmap::IndexMap;

const ADD: StandardFunctions = StandardFunctions::Add;
const IS_NULL: StandardFunctions = StandardFunctions::IsNull;

fn parser() -> ExpressionParser {
    ExpressionParser::new("")
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
    let block = block_utils::new_block(
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
    function: StandardFunctions,
    args: Vec<FederationExpression>,
) -> FunctionCallExpression {
    FunctionCallExpression::new(arrow_type, function.function_name(), args)
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
    parser()
        .parse_complex_expressions(&constraints, &mut acc)
        .unwrap()
}

#[test]
fn test_parse_variable_expression() {
    let expr = VariableExpression::new("column", int32()).unwrap();
    assert_eq!("column", parser().parse_variable_expression(&expr));
}

#[test]
fn test_parse_constant_expression() {
    let expr = constant(2, "val", DataType::Utf8);
    let mut acc = Vec::new();
    let result = parser().parse_constant_expression(&expr, &mut acc).unwrap();
    assert_eq!("?,?", result);
    assert_eq!(2, acc.len());
    assert_eq!(&Value::String("val".to_string()), acc[0].value());
}

#[test]
fn test_parse_function_call_expression_with_all_types() {
    let nested = call(
        int32(),
        ADD,
        vec![
            var("age", int32()),
            FederationExpression::Constant(constant(1, 25, int32())),
        ],
    );
    let mut acc = Vec::new();
    let result = parser()
        .parse_function_call_expression(&nested, &mut acc)
        .unwrap();
    assert_eq!("(age + ?)", result);
    assert_eq!(1, acc.len());
    assert_eq!(&Value::Int(25), acc[0].value());
}

#[test]
fn test_parse_function_call_expression_with_unknown_argument() {
    let function_call = call(
        DataType::Boolean,
        StandardFunctions::And,
        vec![
            var("a", int32()),
            FederationExpression::Constant(constant(1, 1, int32())),
            FederationExpression::FunctionCall(call(int32(), IS_NULL, vec![var("b", int32())])),
        ],
    );
    let mut acc = Vec::new();
    let result = parser()
        .parse_function_call_expression(&function_call, &mut acc)
        .unwrap();
    assert_eq!("(a AND ? AND (b IS NULL))", result);
    let constraints = constraints_with(vec![var("a", int32())]);
    let err = parser()
        .parse_complex_expressions(&constraints, &mut acc)
        .unwrap_err();
    assert!(err.to_string().contains("cannot be cast to class"));
}

#[test]
fn test_parse_complex_expressions_with_empty_constraints() {
    let mut acc = Vec::new();
    assert!(parser()
        .parse_complex_expressions(&Constraints::empty(), &mut acc)
        .unwrap()
        .is_empty());
    assert!(parser()
        .parse_complex_expressions(&constraints_with(Vec::new()), &mut acc)
        .unwrap()
        .is_empty());
}

#[test]
fn test_parse_complex_expressions_single_function() {
    let expressions = parse(vec![call(
        DataType::Boolean,
        IS_NULL,
        vec![var("col", int32())],
    )]);
    assert_eq!(vec!["(col IS NULL)".to_string()], expressions);
}

#[test]
fn test_parse_complex_expressions_multiple_functions() {
    let expressions = parse(vec![
        call(DataType::Boolean, IS_NULL, vec![var("col1", int32())]),
        call(
            DataType::Boolean,
            StandardFunctions::Not,
            vec![var("col2", DataType::Utf8)],
        ),
    ]);
    assert_eq!(
        vec!["(col1 IS NULL)".to_string(), "( NOT col2)".to_string()],
        expressions
    );
}

#[test]
fn test_parse_constant_expression_single_value() {
    let expr = constant(1, 42, int32());
    let mut acc = Vec::new();
    assert_eq!(
        "?",
        parser().parse_constant_expression(&expr, &mut acc).unwrap()
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
        assert_eq!(name, parser().parse_variable_expression(&expr));
    }
}

#[test]
fn test_parse_complex_expressions_with_nested_functions() {
    let subtract = call(
        int32(),
        StandardFunctions::Subtract,
        vec![var("col1", int32()), var("col2", int32())],
    );
    let multiply = call(
        int32(),
        StandardFunctions::Multiply,
        vec![var("col1", int32()), var("col2", int32())],
    );
    let add = call(
        int32(),
        ADD,
        vec![
            FederationExpression::FunctionCall(subtract),
            FederationExpression::FunctionCall(multiply),
        ],
    );
    let expressions = parse(vec![add]);
    assert_eq!(
        vec!["((col1 - col2) + (col1 * col2))".to_string()],
        expressions
    );
}

#[test]
fn test_parse_complex_expressions_with_case_when() {
    let greater = call(
        DataType::Boolean,
        StandardFunctions::GreaterThan,
        vec![
            var("age", int32()),
            FederationExpression::Constant(constant(1, 18, int32())),
        ],
    );
    let case = call(
        int32(),
        StandardFunctions::Or,
        vec![
            FederationExpression::FunctionCall(greater),
            var("salary", int32()),
            var("bonus", int32()),
        ],
    );
    let expressions = parse(vec![case]);
    assert_eq!(
        vec!["((age > ?) OR salary OR bonus)".to_string()],
        expressions
    );
}

#[test]
fn test_parse_complex_expressions_with_aggregate_functions() {
    let add = call(
        int32(),
        ADD,
        vec![var("col1", int32()), var("col1", int32())],
    );
    let sum = call(
        int32(),
        StandardFunctions::Negate,
        vec![FederationExpression::FunctionCall(add)],
    );
    let expressions = parse(vec![sum]);
    assert_eq!(vec!["(-(col1 + col1))".to_string()], expressions);
}

#[test]
fn test_parse_complex_expressions_with_string_functions() {
    let like = call(
        DataType::Boolean,
        StandardFunctions::LikePattern,
        vec![var("name", DataType::Utf8), var("pattern", DataType::Utf8)],
    );
    let concat = call(
        DataType::Utf8,
        ADD,
        vec![
            var("name", DataType::Utf8),
            FederationExpression::Constant(constant(1, "_suffix", DataType::Utf8)),
        ],
    );
    let expressions = parse(vec![like, concat]);
    assert_eq!(
        vec!["(name LIKE pattern)".to_string(), "(name + ?)".to_string()],
        expressions
    );
}

#[test]
fn test_parse_complex_expressions_with_null_handling() {
    let coalesce = call(
        int32(),
        StandardFunctions::And,
        vec![
            var("col1", int32()),
            var("col2", int32()),
            FederationExpression::Constant(constant(1, 0, int32())),
        ],
    );
    let is_null = call(DataType::Boolean, IS_NULL, vec![var("col1", int32())]);
    let expressions = parse(vec![coalesce, is_null]);
    assert_eq!(
        vec![
            "(col1 AND col2 AND ?)".to_string(),
            "(col1 IS NULL)".to_string()
        ],
        expressions
    );
}
