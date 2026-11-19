use std::collections::HashMap;

use arrow::datatypes::DataType;

use athena_jdbc::sql::JdbcType;
use athena_snowflake::utils::SnowflakeArrowTypeConverter;

fn convert(
    jdbc_type: JdbcType,
    precision: i32,
    scale: i32,
    config: &HashMap<String, String>,
) -> DataType {
    SnowflakeArrowTypeConverter::to_arrow_type(jdbc_type, precision, scale, config)
        .unwrap()
        .unwrap()
}

#[test]
fn test_to_arrow_type_integer() {
    let result = convert(JdbcType::Integer, 10, 0, &HashMap::new());
    assert!(result.is_integer());
    assert_eq!(result, DataType::Int32);
}

#[test]
fn test_to_arrow_type_varchar() {
    assert_eq!(
        convert(JdbcType::Varchar, 255, 0, &HashMap::new()),
        DataType::Utf8
    );
}

#[test]
fn test_to_arrow_type_big_int() {
    let expected_precision = 19;
    let result = convert(JdbcType::BigInt, 19, 0, &HashMap::new());
    let DataType::Decimal128(precision, _) = result else {
        panic!("expected a decimal type, got {result:?}");
    };
    assert_eq!(precision, expected_precision);
    assert_eq!(result, DataType::Decimal128(19, 0));
}

#[test]
fn test_to_arrow_type_numeric_with_default_scale() {
    let config = HashMap::from([("default_scale".to_string(), "2".to_string())]);
    let result = convert(JdbcType::Numeric, 0, 0, &config);
    let DataType::Decimal128(precision, scale) = result else {
        panic!("expected a decimal type, got {result:?}");
    };
    assert_eq!(precision, 38);
    assert_eq!(scale, 2);
}

#[test]
fn test_to_arrow_type_decimal_exceeding_precision() {
    let result = convert(JdbcType::Decimal, 50, 10, &HashMap::new());
    let DataType::Decimal128(precision, _) = result else {
        panic!("expected a decimal type, got {result:?}");
    };
    assert_eq!(precision, 38);
}

#[test]
fn test_to_arrow_type_timestamp_with_timezone() {
    let result = convert(JdbcType::TimestampWithTimezone, 0, 0, &HashMap::new());
    assert!(matches!(result, DataType::Date32 | DataType::Date64));
    assert_eq!(result, DataType::Date64);
}

#[test]
fn test_to_arrow_type_array_type() {
    let result = convert(JdbcType::Array, 0, 0, &HashMap::new());
    assert!(matches!(result, DataType::List(_)));
}

#[test]
fn test_to_arrow_type_with_null_config_options() {
    let result =
        SnowflakeArrowTypeConverter::to_arrow_type(JdbcType::Integer, 10, 0, &HashMap::new());
    assert_eq!(result.unwrap(), Some(DataType::Int32));
}
