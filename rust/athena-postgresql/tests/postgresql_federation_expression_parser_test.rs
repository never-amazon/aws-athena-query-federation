use arrow::datatypes::DataType;
use athena_federation_sdk::data::ArrowType;
use athena_jdbc::query::ExpressionParser;

fn write(_arrow_type: &ArrowType, arguments: &[&str]) -> String {
    let parser = ExpressionParser::new("\"");
    let arguments: Vec<String> = arguments.iter().map(ToString::to_string).collect();
    parser.write_array_constructor_clause(&arguments)
}

#[test]
fn test_write_array_constructor_clause() {
    assert_eq!(
        "'value1', 'value2', 'value3'",
        write(&DataType::Utf8, &["'value1'", "'value2'", "'value3'"])
    );
}

#[test]
fn test_write_array_constructor_clause_with_numbers() {
    assert_eq!("1, 2, 3", write(&DataType::Int32, &["1", "2", "3"]));
}

#[test]
fn test_write_array_constructor_clause_with_empty_list() {
    assert_eq!("", write(&DataType::Utf8, &[]));
}

#[test]
fn test_write_array_constructor_clause_with_single_element() {
    assert_eq!("'single'", write(&DataType::Utf8, &["'single'"]));
}

#[test]
fn test_write_array_constructor_clause_with_float() {
    assert_eq!(
        "1.5, 2.75, -3.25",
        write(&DataType::Float32, &["1.5", "2.75", "-3.25"])
    );
}

#[test]
fn test_write_array_constructor_clause_with_double() {
    assert_eq!(
        "1.123456789, -2.987654321, 3.14159265359",
        write(
            &DataType::Float64,
            &["1.123456789", "-2.987654321", "3.14159265359"]
        )
    );
}

#[test]
fn test_write_array_constructor_clause_with_boolean() {
    assert_eq!(
        "true, false, true",
        write(&DataType::Boolean, &["true", "false", "true"])
    );
}

#[test]
fn test_write_array_constructor_clause_with_big_int() {
    assert_eq!(
        "9223372036854775807, -9223372036854775808, 0",
        write(
            &DataType::Int64,
            &["9223372036854775807", "-9223372036854775808", "0"]
        )
    );
}

#[test]
fn test_write_array_constructor_clause_with_special_characters() {
    assert_eq!(
        "'Hello, World!', 'It''s working', 'Semi;Colon'",
        write(
            &DataType::Utf8,
            &["'Hello, World!'", "'It''s working'", "'Semi;Colon'"]
        )
    );
}

#[test]
fn test_write_array_constructor_clause_with_date() {
    assert_eq!(
        "'2024-03-14', '2024-12-31', '2025-01-01'",
        write(
            &DataType::Date32,
            &["'2024-03-14'", "'2024-12-31'", "'2025-01-01'"]
        )
    );
}
