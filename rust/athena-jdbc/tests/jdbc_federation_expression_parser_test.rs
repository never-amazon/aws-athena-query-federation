use arrow::datatypes::DataType;
use athena_federation_sdk::domain::predicate::{FunctionName, VariableExpression};
use athena_federation_sdk::exceptions::FederationSourceErrorCode;
use athena_jdbc::manager::ExpressionParser;

fn parser() -> ExpressionParser {
    ExpressionParser::new("\"")
}

fn map(function_name: &str, _kind: &str, arguments: &[String]) -> String {
    parser()
        .map_function_to_data_source_syntax(&FunctionName::new(function_name), arguments)
        .unwrap()
}

fn assert_athena_connector_exception(
    function_name: &str,
    arguments: &[String],
    expected_message: &str,
) {
    let err = parser()
        .map_function_to_data_source_syntax(&FunctionName::new(function_name), arguments)
        .unwrap_err();
    assert_eq!(
        Some(FederationSourceErrorCode::InvalidInputException),
        err.error_code()
    );
    assert_eq!(expected_message, err.to_string());
}

#[test]
fn test_parse_variable_expression() {
    let expr = VariableExpression::new("my_col", DataType::Int32).unwrap();
    assert_eq!("\"my_col\"", parser().parse_variable_expression(&expr));
}

#[test]
fn test_add_function() {
    assert_eq!(
        "(a + b)",
        map("$add", "int", &["a".to_string(), "b".to_string()])
    );
}

#[test]
fn test_and_function() {
    assert_eq!(
        "(a AND b)",
        map("$and", "null", &["a".to_string(), "b".to_string()])
    );
}

#[test]
fn test_in_predicate() {
    assert_eq!(
        "(a IN (1, 2, 3))",
        map("$in", "null", &["a".to_string(), "(1, 2, 3)".to_string()])
    );
}

#[test]
fn test_unary_function_valid() {
    assert_eq!("(x IS NULL)", map("$is_null", "null", &["x".to_string()]));
}

#[test]
fn test_binary_function_valid() {
    assert_eq!(
        "(x = y)",
        map("$equal", "null", &["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_negate_function() {
    assert_eq!("(-5)", map("$negate", "null", &["5".to_string()]));
}

#[test]
fn test_null_if_function() {
    assert_eq!(
        "(NULLIF(a, b))",
        map("$nullif", "null", &["a".to_string(), "b".to_string()])
    );
}

#[test]
fn test_write_array_constructor_function() {
    assert_eq!(
        "(1, 2, 3)",
        map(
            "$array",
            "int",
            &["1".to_string(), "2".to_string(), "3".to_string()]
        )
    );
}

#[test]
fn test_not_equal_function() {
    assert_eq!(
        "(x <> y)",
        map("$not_equal", "null", &["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_greater_than_function() {
    assert_eq!(
        "(x > y)",
        map("$greater_than", "null", &["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_greater_than_or_equal_function() {
    assert_eq!(
        "(x >= y)",
        map(
            "$greater_than_or_equal",
            "null",
            &["x".to_string(), "y".to_string()]
        )
    );
}

#[test]
fn test_less_than_function() {
    assert_eq!(
        "(x < y)",
        map("$less_than", "null", &["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_less_than_or_equal_function() {
    assert_eq!(
        "(x <= y)",
        map(
            "$less_than_or_equal",
            "null",
            &["x".to_string(), "y".to_string()]
        )
    );
}

#[test]
fn test_not_function() {
    assert_eq!("( NOT x)", map("$not", "null", &["x".to_string()]));
}

#[test]
fn test_or_function() {
    assert_eq!(
        "(x OR y)",
        map("$or", "null", &["x".to_string(), "y".to_string()])
    );
}

#[test]
fn test_mod_function() {
    assert_eq!(
        "(10 % 3)",
        map("$modulus", "int", &["10".to_string(), "3".to_string()])
    );
}

#[test]
fn test_multiply_function() {
    assert_eq!(
        "(2 * 3)",
        map("$multiply", "int", &["2".to_string(), "3".to_string()])
    );
}

#[test]
fn test_divide_function() {
    assert_eq!(
        "(6 / 2)",
        map("$divide", "int", &["6".to_string(), "2".to_string()])
    );
}

#[test]
fn test_subtract_function() {
    assert_eq!(
        "(7 - 4)",
        map("$subtract", "int", &["7".to_string(), "4".to_string()])
    );
}

#[test]
fn test_like_function() {
    assert_eq!(
        "(name LIKE 'J%')",
        map(
            "$like_pattern",
            "null",
            &["name".to_string(), "'J%'".to_string()]
        )
    );
}

#[test]
fn test_complex_nested_functions() {
    assert_eq!(
        "('hello', 'world', 'test')",
        map(
            "$array",
            "utf8",
            &[
                "'hello'".to_string(),
                "'world'".to_string(),
                "'test'".to_string()
            ]
        )
    );
}

#[test]
fn test_arithmetic_functions_with_different_types() {
    assert_eq!(
        "(3.14 + 2.86)",
        map("$add", "double", &["3.14".to_string(), "2.86".to_string()])
    );
    assert_eq!(
        "(100.50 * 2)",
        map(
            "$multiply",
            "decimal",
            &["100.50".to_string(), "2".to_string()]
        )
    );
}

#[test]
fn test_logical_functions_with_multiple_arguments() {
    assert_eq!(
        "(condition1 AND condition2)",
        map(
            "$and",
            "null",
            &["condition1".to_string(), "condition2".to_string()]
        )
    );
    assert_eq!(
        "(condition1 OR condition2)",
        map(
            "$or",
            "null",
            &["condition1".to_string(), "condition2".to_string()]
        )
    );
}

#[test]
fn test_comparison_functions_edge_cases() {
    assert_eq!(
        "(age > 18)",
        map(
            "$greater_than",
            "null",
            &["age".to_string(), "18".to_string()]
        )
    );
    assert_eq!(
        "(score >= 90)",
        map(
            "$greater_than_or_equal",
            "null",
            &["score".to_string(), "90".to_string()]
        )
    );
    assert_eq!(
        "(count < 100)",
        map(
            "$less_than",
            "null",
            &["count".to_string(), "100".to_string()]
        )
    );
    assert_eq!(
        "(limit <= 50)",
        map(
            "$less_than_or_equal",
            "null",
            &["limit".to_string(), "50".to_string()]
        )
    );
}

#[test]
fn test_unary_functions_edge_cases() {
    assert_eq!(
        "( NOT active)",
        map("$not", "null", &["active".to_string()])
    );
    assert_eq!("(-3.14)", map("$negate", "null", &["3.14".to_string()]));
}

#[test]
fn test_complex_nested_arithmetic_expressions() {
    assert_eq!(
        "(((a + b) * c) + ((d - e) / f))",
        map(
            "$add",
            "int",
            &["((a + b) * c)".to_string(), "((d - e) / f)".to_string()]
        )
    );
}

#[test]
fn test_complex_logical_expressions() {
    assert_eq!(
        "((age > 18) AND (salary > 50000) AND (department = 'IT'))",
        map(
            "$and",
            "null",
            &[
                "(age > 18)".to_string(),
                "(salary > 50000)".to_string(),
                "(department = 'IT')".to_string()
            ]
        )
    );
    assert_eq!(
        "((status = 'active') OR (status = 'pending') OR (status = 'approved'))",
        map(
            "$or",
            "null",
            &[
                "(status = 'active')".to_string(),
                "(status = 'pending')".to_string(),
                "(status = 'approved')".to_string()
            ]
        )
    );
}

#[test]
fn test_complex_string_expressions() {
    assert_eq!(
        "(email LIKE '%@company.com')",
        map(
            "$like_pattern",
            "null",
            &["email".to_string(), "'%@company.com'".to_string()]
        )
    );
    assert_eq!(
        "((name LIKE 'J%') AND (age > 18))",
        map(
            "$and",
            "null",
            &["(name LIKE 'J%')".to_string(), "(age > 18)".to_string()]
        )
    );
}

#[test]
fn test_complex_date_expressions() {
    assert_eq!(
        "(order_date + 30)",
        map("$add", "int", &["order_date".to_string(), "30".to_string()])
    );
    assert_eq!(
        "(end_date - start_date)",
        map(
            "$subtract",
            "int",
            &["end_date".to_string(), "start_date".to_string()]
        )
    );
}

#[test]
fn test_complex_aggregate_expressions() {
    assert_eq!(
        "(price * quantity)",
        map(
            "$multiply",
            "int",
            &["price".to_string(), "quantity".to_string()]
        )
    );
    assert_eq!(
        "(score / total_questions)",
        map(
            "$divide",
            "double",
            &["score".to_string(), "total_questions".to_string()]
        )
    );
}

#[test]
fn test_complex_case_expressions() {
    assert_eq!(
        "(age >= 65)",
        map(
            "$greater_than_or_equal",
            "null",
            &["age".to_string(), "65".to_string()]
        )
    );
    assert_eq!(
        "(age <= 18)",
        map(
            "$less_than_or_equal",
            "null",
            &["age".to_string(), "18".to_string()]
        )
    );
}

#[test]
fn test_complex_null_handling_expressions() {
    assert_eq!(
        "(NULLIF((salary * 1.1), 0))",
        map(
            "$nullif",
            "int",
            &["(salary * 1.1)".to_string(), "0".to_string()]
        )
    );
    assert_eq!(
        "(salary * 1.1)",
        map(
            "$multiply",
            "int",
            &["salary".to_string(), "1.1".to_string()]
        )
    );
}

#[test]
fn test_complex_window_function_expressions() {
    assert_eq!(
        "(salary + bonus)",
        map("$add", "int", &["salary".to_string(), "bonus".to_string()])
    );
    assert_eq!(
        "((department = 'IT') AND (salary > 50000))",
        map(
            "$and",
            "null",
            &[
                "(department = 'IT')".to_string(),
                "(salary > 50000)".to_string()
            ]
        )
    );
}

#[test]
fn test_empty_arguments_throws_exception() {
    assert_athena_connector_exception("$add", &[], "Arguments cannot be null or empty.");
}

#[test]
fn test_unary_function_invalid_arg_count() {
    assert_athena_connector_exception(
        "$is_null",
        &["a".to_string(), "b".to_string()],
        "Unary function type $is_null was provided with 2 arguments.",
    );
}

#[test]
fn test_binary_function_invalid_arg_count() {
    assert_athena_connector_exception(
        "$add",
        &["a".to_string()],
        "Binary function type $add was provided with 1 arguments.",
    );
}

#[test]
fn test_null_arguments() {
    assert_athena_connector_exception("$add", &[], "Arguments cannot be null or empty.");
}

#[test]
fn test_ternary_function_invalid_arg_count() {
    assert_athena_connector_exception(
        "$nullif",
        &["a".to_string(), "b".to_string(), "x".to_string()],
        "Binary function type $nullif was provided with 3 arguments.",
    );
}
