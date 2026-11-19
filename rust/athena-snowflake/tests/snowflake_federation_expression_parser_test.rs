use arrow::datatypes::{DataType, TimeUnit};

use athena_federation_sdk::domain::predicate::{
    ConstantExpression, FederationExpression, FunctionCallExpression, StandardFunctions,
    VariableExpression,
};
use athena_federation_sdk::FederationError;
use athena_jdbc::manager::ExpressionParser;

const QUOTE_CHAR_DOUBLE: &str = "\"";
const QUOTE_CHAR_BACKTICK: &str = "`";
const TEST_ARG_1: &str = "arg1";
const TEST_ARG_2: &str = "arg2";
const TEST_ARG_3: &str = "arg3";
const TEST_ARG_4: &str = "arg4";
const TEST_ARG_5: &str = "arg5";
const TEST_ARG_6: &str = "arg6";
const TEST_ARG_7: &str = "arg7";
const TEST_ARG_8: &str = "arg8";
const TEST_ARG_9: &str = "arg9";
const TEST_ARG_10: &str = "arg10";
const TEST_ARG_WITH_COMMA: &str = "arg2, with comma";
const TEST_ARG_SPECIAL: &str = "test_arg";
const EXPECTED_SINGLE_ARG: &str = "test_arg";
const EXPECTED_TWO_ARGS: &str = "arg1, arg2";
const EXPECTED_THREE_ARGS: &str = "arg1, arg2, arg3";
const EXPECTED_TEN_ARGS: &str = "arg1, arg2, arg3, arg4, arg5, arg6, arg7, arg8, arg9, arg10";
const EXPECTED_MIXED_ARGS: &str = "string, 123, true, null";
const EXPECTED_SPECIAL_CHARS: &str = "arg1, arg2, with comma, arg3";
const EXPECTED_NUMBERS: &str = "1, 2, 3";
const EXPECTED_EMPTY_STRINGS: &str = ", arg2, ";
const EXPECTED_WHITESPACE: &str = " arg1 ,  arg2 ,  arg3 ";

fn parser() -> ExpressionParser {
    ExpressionParser::new(QUOTE_CHAR_DOUBLE)
}

fn parse_variable(parser: &ExpressionParser, column: &str) -> String {
    let expression = VariableExpression::new(column, DataType::Utf8).unwrap();
    parser.parse_variable_expression(&expression)
}

fn strings(arguments: &[&str]) -> Vec<String> {
    arguments.iter().map(ToString::to_string).collect()
}

fn write_clause(_arrow_type: &DataType, arguments: &[&str]) -> String {
    parser().write_array_constructor_clause(&strings(arguments))
}

#[test]
fn constructor_with_double_quote_char_wraps_column_names_correctly() {
    let parser = ExpressionParser::new(QUOTE_CHAR_DOUBLE);
    assert_eq!(parse_variable(&parser, "testColumn"), "\"testColumn\"");
    assert_eq!(parse_variable(&parser, "column_name"), "\"column_name\"");
    assert_eq!(parse_variable(&parser, "MyColumn"), "\"MyColumn\"");
}

#[test]
fn parse_variable_expression_with_double_quote_char_wraps_column_names_correctly() {
    assert_eq!(parse_variable(&parser(), "testColumn"), "\"testColumn\"");
}

#[test]
fn constructor_with_backtick_quote_char_wraps_column_names_correctly() {
    let parser = ExpressionParser::new(QUOTE_CHAR_BACKTICK);
    assert_eq!(parse_variable(&parser, "testColumn"), "`testColumn`");
    assert_eq!(parse_variable(&parser, "column_name"), "`column_name`");
}

#[test]
fn constructor_with_single_quote_char_wraps_column_names_correctly() {
    let parser = ExpressionParser::new("'");
    assert_eq!(parse_variable(&parser, "testColumn"), "'testColumn'");
    assert_eq!(parse_variable(&parser, "MyColumn"), "'MyColumn'");
}

#[test]
fn write_array_constructor_clause_with_single_argument_returns_correct_string() {
    assert_eq!(
        write_clause(&DataType::Utf8, &[TEST_ARG_SPECIAL]),
        EXPECTED_SINGLE_ARG
    );
}

#[test]
fn write_array_constructor_clause_with_multiple_arguments_returns_comma_separated_string() {
    assert_eq!(
        write_clause(&DataType::Utf8, &[TEST_ARG_1, TEST_ARG_2, TEST_ARG_3]),
        EXPECTED_THREE_ARGS
    );
}

#[test]
fn write_array_constructor_clause_with_empty_list_returns_empty_string() {
    assert_eq!(write_clause(&DataType::Utf8, &[]), "");
}

#[test]
fn write_array_constructor_clause_with_null_arguments_throws_null_pointer_exception() {
    let parser = parser();
    let variable = |name: &str| {
        FederationExpression::Variable(VariableExpression::new(name, DataType::Utf8).unwrap())
    };
    let call = FunctionCallExpression::new(
        DataType::Utf8,
        StandardFunctions::ArrayConstructor.function_name(),
        vec![
            variable(TEST_ARG_1),
            FederationExpression::Constant(ConstantExpression::new(None, DataType::Utf8)),
            variable(TEST_ARG_2),
        ],
    );
    let error = parser
        .parse_function_call_expression(&call, &mut Vec::new())
        .unwrap_err();
    assert!(
        matches!(error, FederationError::NullPointer(_)),
        "expected NullPointer, got {error:?}"
    );
    assert_eq!(
        "Cannot invoke \"com.amazonaws.athena.connector.lambda.data.Block.getFieldReader(String)\" because \"values\" is null",
        error.to_string()
    );
}

#[test]
fn write_array_constructor_clause_with_different_arrow_types_returns_comma_separated_string() {
    let types = [
        DataType::Int32,
        DataType::Float64,
        DataType::Boolean,
        DataType::Date32,
        DataType::Timestamp(TimeUnit::Millisecond, None),
    ];
    for arrow_type in &types {
        assert_eq!(
            write_clause(arrow_type, &[TEST_ARG_1, TEST_ARG_2]),
            EXPECTED_TWO_ARGS
        );
    }
}

#[test]
fn write_array_constructor_clause_with_special_characters_returns_comma_separated_string() {
    assert_eq!(
        write_clause(
            &DataType::Utf8,
            &[TEST_ARG_1, TEST_ARG_WITH_COMMA, TEST_ARG_3]
        ),
        EXPECTED_SPECIAL_CHARS
    );
}

#[test]
fn write_array_constructor_clause_with_numbers_returns_comma_separated_string() {
    assert_eq!(
        write_clause(&DataType::Int32, &["1", "2", "3"]),
        EXPECTED_NUMBERS
    );
}

#[test]
fn write_array_constructor_clause_with_mixed_types_returns_comma_separated_string() {
    assert_eq!(
        write_clause(&DataType::Utf8, &["string", "123", "true", "null"]),
        EXPECTED_MIXED_ARGS
    );
}

#[test]
fn write_array_constructor_clause_with_large_list_returns_comma_separated_string() {
    let arguments = [
        TEST_ARG_1,
        TEST_ARG_2,
        TEST_ARG_3,
        TEST_ARG_4,
        TEST_ARG_5,
        TEST_ARG_6,
        TEST_ARG_7,
        TEST_ARG_8,
        TEST_ARG_9,
        TEST_ARG_10,
    ];
    assert_eq!(write_clause(&DataType::Utf8, &arguments), EXPECTED_TEN_ARGS);
}

#[test]
fn write_array_constructor_clause_with_empty_strings_returns_comma_separated_string_with_empty_values(
) {
    assert_eq!(
        write_clause(&DataType::Utf8, &["", TEST_ARG_2, ""]),
        EXPECTED_EMPTY_STRINGS
    );
}

#[test]
fn write_array_constructor_clause_with_whitespace_returns_comma_separated_string_with_whitespace() {
    assert_eq!(
        write_clause(&DataType::Utf8, &[" arg1 ", " arg2 ", " arg3 "]),
        EXPECTED_WHITESPACE
    );
}
