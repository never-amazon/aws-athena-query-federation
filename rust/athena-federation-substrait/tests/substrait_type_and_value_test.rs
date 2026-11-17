#![allow(clippy::approx_constant)]

use athena_federation_substrait::ErrorKind;
use athena_federation_substrait::SqlTypeName;
use athena_federation_substrait::{SubstraitTypeAndValue, SubstraitValue};
use bigdecimal::BigDecimal;
use std::str::FromStr;

fn tv(t: SqlTypeName, v: SubstraitValue, c: &str) -> SubstraitTypeAndValue {
    SubstraitTypeAndValue::new(t, v, c).expect("valid")
}

#[test]
fn test_constructor_with_valid_parameters() {
    let tav = tv(
        SqlTypeName::Integer,
        SubstraitValue::Integer(123),
        "test_column",
    );
    assert_eq!(tav.get_type(), SqlTypeName::Integer);
    assert_eq!(tav.get_value(), &SubstraitValue::Integer(123));
    assert_eq!(tav.get_column_name(), "test_column");
}

#[test]
fn test_constructor_with_null_type() {
    let err = SubstraitTypeAndValue::builder()
        .value(SubstraitValue::Integer(123))
        .column_name("test_column")
        .build()
        .expect_err("null type");
    assert_eq!(err.kind(), ErrorKind::NullPointer);
    assert_eq!(err.message(), "type is null");
}

#[test]
fn test_constructor_with_null_value() {
    let err = SubstraitTypeAndValue::new(SqlTypeName::Integer, SubstraitValue::Null, "test_column")
        .expect_err("null value");
    assert_eq!(err.kind(), ErrorKind::NullPointer);
    assert_eq!(err.message(), "value is null");
}

#[test]
fn test_constructor_with_null_column_name() {
    let err = SubstraitTypeAndValue::builder()
        .type_name(SqlTypeName::Integer)
        .value(SubstraitValue::Integer(123))
        .build()
        .expect_err("null column");
    assert_eq!(err.kind(), ErrorKind::NullPointer);
    assert_eq!(err.message(), "value is null");
}

#[test]
fn test_getters_with_integer_type() {
    let value = SubstraitValue::Integer(456);
    let tav = tv(SqlTypeName::Integer, value.clone(), "int_column");
    assert_eq!(tav.get_type(), SqlTypeName::Integer);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "int_column");
}

#[test]
fn test_getters_with_big_int_type() {
    let value = SubstraitValue::Long(9223372036854775807);
    let tav = tv(SqlTypeName::Bigint, value.clone(), "bigint_column");
    assert_eq!(tav.get_type(), SqlTypeName::Bigint);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "bigint_column");
}

#[test]
fn test_getters_with_varchar_type() {
    let value = SubstraitValue::String("test_string".to_string());
    let tav = tv(SqlTypeName::Varchar, value.clone(), "varchar_column");
    assert_eq!(tav.get_type(), SqlTypeName::Varchar);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "varchar_column");
}

#[test]
fn test_getters_with_double_type() {
    let value = SubstraitValue::Double(3.141592653589793);
    let tav = tv(SqlTypeName::Double, value.clone(), "double_column");
    assert_eq!(tav.get_type(), SqlTypeName::Double);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "double_column");
}

#[test]
fn test_getters_with_float_type() {
    let value = SubstraitValue::Float(3.14f32);
    let tav = tv(SqlTypeName::Float, value.clone(), "float_column");
    assert_eq!(tav.get_type(), SqlTypeName::Float);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "float_column");
}

#[test]
fn test_getters_with_boolean_type() {
    let value = SubstraitValue::Boolean(true);
    let tav = tv(SqlTypeName::Boolean, value.clone(), "boolean_column");
    assert_eq!(tav.get_type(), SqlTypeName::Boolean);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "boolean_column");
}

#[test]
fn test_getters_with_decimal_type() {
    let value = SubstraitValue::Decimal(BigDecimal::from_str("123.45").expect("decimal"));
    let tav = tv(SqlTypeName::Decimal, value.clone(), "decimal_column");
    assert_eq!(tav.get_type(), SqlTypeName::Decimal);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "decimal_column");
}

#[test]
fn test_getters_with_date_type() {
    let value = SubstraitValue::String("2024-01-01".to_string());
    let tav = tv(SqlTypeName::Date, value.clone(), "date_column");
    assert_eq!(tav.get_type(), SqlTypeName::Date);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "date_column");
}

#[test]
fn test_getters_with_time_type() {
    let value = SubstraitValue::String("12:30:45".to_string());
    let tav = tv(SqlTypeName::Time, value.clone(), "time_column");
    assert_eq!(tav.get_type(), SqlTypeName::Time);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "time_column");
}

#[test]
fn test_getters_with_timestamp_type() {
    let value = SubstraitValue::String("2024-01-01 12:30:45".to_string());
    let tav = tv(SqlTypeName::Timestamp, value.clone(), "timestamp_column");
    assert_eq!(tav.get_type(), SqlTypeName::Timestamp);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "timestamp_column");
}

#[test]
fn test_getters_with_varbinary_type() {
    let value = SubstraitValue::Bytes(b"test_bytes".to_vec());
    let tav = tv(SqlTypeName::Varbinary, value.clone(), "varbinary_column");
    assert_eq!(tav.get_type(), SqlTypeName::Varbinary);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "varbinary_column");
}

#[test]
fn test_getters_with_tiny_int_type() {
    let value = SubstraitValue::Byte(127);
    let tav = tv(SqlTypeName::Tinyint, value.clone(), "tinyint_column");
    assert_eq!(tav.get_type(), SqlTypeName::Tinyint);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "tinyint_column");
}

#[test]
fn test_getters_with_small_int_type() {
    let value = SubstraitValue::Short(32767);
    let tav = tv(SqlTypeName::Smallint, value.clone(), "smallint_column");
    assert_eq!(tav.get_type(), SqlTypeName::Smallint);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "smallint_column");
}

#[test]
fn test_getters_with_char_type() {
    let value = SubstraitValue::String("A".to_string());
    let tav = tv(SqlTypeName::Char, value.clone(), "char_column");
    assert_eq!(tav.get_type(), SqlTypeName::Char);
    assert_eq!(tav.get_value(), &value);
    assert_eq!(tav.get_column_name(), "char_column");
}

#[test]
fn test_to_string() {
    let result = tv(
        SqlTypeName::Integer,
        SubstraitValue::Integer(123),
        "test_column",
    )
    .to_string();
    assert!(result.contains("TypeAndValue"));
    assert!(result.contains("type=INTEGER"));
    assert!(result.contains("value=123"));
    assert_eq!(result, "TypeAndValue{type=INTEGER, value=123}");
}

#[test]
fn test_to_string_with_string_value() {
    let result = tv(
        SqlTypeName::Varchar,
        SubstraitValue::String("hello world".to_string()),
        "varchar_column",
    )
    .to_string();
    assert!(result.contains("TypeAndValue"));
    assert!(result.contains("type=VARCHAR"));
    assert!(result.contains("value=hello world"));
}

#[test]
fn test_to_string_with_complex_value() {
    let value = SubstraitValue::Decimal(BigDecimal::from_str("999.999").expect("decimal"));
    let result = tv(SqlTypeName::Decimal, value, "decimal_column").to_string();
    assert!(result.contains("TypeAndValue"));
    assert!(result.contains("type=DECIMAL"));
    assert!(result.contains("value=999.999"));
}

#[test]
fn test_multiple_instances_with_same_values() {
    let a = tv(
        SqlTypeName::Integer,
        SubstraitValue::Integer(42),
        "test_column",
    );
    let b = tv(
        SqlTypeName::Integer,
        SubstraitValue::Integer(42),
        "test_column",
    );
    assert_eq!(a.get_type(), b.get_type());
    assert_eq!(a.get_value(), b.get_value());
    assert_eq!(a.get_column_name(), b.get_column_name());
}

#[test]
fn test_multiple_instances_with_different_values() {
    let a = tv(SqlTypeName::Integer, SubstraitValue::Integer(123), "col1");
    let b = tv(
        SqlTypeName::Varchar,
        SubstraitValue::String("test".to_string()),
        "col2",
    );
    assert_eq!(a.get_type(), SqlTypeName::Integer);
    assert_eq!(b.get_type(), SqlTypeName::Varchar);
    assert_eq!(a.get_value(), &SubstraitValue::Integer(123));
    assert_eq!(b.get_value(), &SubstraitValue::String("test".to_string()));
    assert_eq!(a.get_column_name(), "col1");
    assert_eq!(b.get_column_name(), "col2");
}
