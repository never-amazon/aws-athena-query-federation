use std::sync::Arc;

use arrow::datatypes::{DataType, IntervalUnit, TimeUnit};
use athena_federation_sdk::data::{ArrowTypeComparator, Value};
use athena_federation_sdk::FederationError;
use bigdecimal::BigDecimal;
use chrono::NaiveDate;
use std::str::FromStr;

const STRING_ABC: &str = "abc";
const STRING_XYZ: &str = "xyz";

fn decimal1() -> Value {
    Value::Decimal(BigDecimal::from_str("123.45").unwrap())
}

fn decimal2() -> Value {
    Value::Decimal(BigDecimal::from_str("678.90").unwrap())
}

fn binary1() -> Value {
    Value::Bytes(vec![0x01, 0x02])
}

fn binary2() -> Value {
    Value::Bytes(vec![0x01, 0x03])
}

fn date1() -> Value {
    Value::LocalDateTime(
        NaiveDate::from_ymd_opt(2023, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
    )
}

fn date2() -> Value {
    Value::LocalDateTime(
        NaiveDate::from_ymd_opt(2023, 1, 2)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
    )
}

fn struct1() -> Value {
    Value::struct_of([("key1", Value::from("value1")), ("key2", Value::Int(42))])
}

fn struct2() -> Value {
    Value::struct_of([("key1", Value::from("value1")), ("key2", Value::Int(43))])
}

fn int_type() -> DataType {
    DataType::Int32
}

fn date_milli_type() -> DataType {
    DataType::Timestamp(TimeUnit::Millisecond, Some(Arc::from("Z")))
}

fn struct_type() -> DataType {
    DataType::Struct(arrow::datatypes::Fields::default())
}

fn unsupported_type() -> DataType {
    DataType::Interval(IntervalUnit::YearMonth)
}

fn assert_comparison(
    expected: i32,
    arrow_type: &DataType,
    lhs: impl Into<Value>,
    rhs: impl Into<Value>,
) {
    assert_eq!(
        expected,
        ArrowTypeComparator::compare(arrow_type, &lhs.into(), &rhs.into()).unwrap()
    );
}

fn assert_struct_comparison(expected: i32, lhs: &Value, rhs: &Value) {
    assert_eq!(
        expected,
        ArrowTypeComparator::compare(&struct_type(), lhs, rhs).unwrap()
    );
}

#[test]
fn test_compare_integers() {
    assert_comparison(-1, &int_type(), 5, 10);
    assert_comparison(1, &int_type(), 10, 5);
    assert_comparison(0, &int_type(), 5, 5);
}

#[test]
fn test_compare_tiny_int() {
    assert_comparison(-1, &DataType::Int8, 1i8, 2i8);
    assert_comparison(1, &DataType::Int8, 2i8, 1i8);
    assert_comparison(0, &DataType::Int8, 1i8, 1i8);
}

#[test]
fn test_compare_small_int() {
    assert_comparison(-100, &DataType::Int16, 100i16, 200i16);
    assert_comparison(100, &DataType::Int16, 200i16, 100i16);
    assert_comparison(0, &DataType::Int16, 100i16, 100i16);
}

#[test]
fn test_compare_uint2() {
    let a = Value::Char('A' as u16);
    let b = Value::Char('B' as u16);
    assert_comparison(-1, &DataType::UInt16, a.clone(), b.clone());
    assert_comparison(1, &DataType::UInt16, b, a.clone());
    assert_comparison(0, &DataType::UInt16, a.clone(), a);
}

#[test]
fn test_compare_big_int() {
    assert_comparison(-1, &DataType::Int64, 1000i64, 2000i64);
    assert_comparison(1, &DataType::Int64, 2000i64, 1000i64);
    assert_comparison(0, &DataType::Int64, 1000i64, 1000i64);
}

#[test]
fn test_compare_float8() {
    assert_comparison(-1, &DataType::Float64, 123.45f64, 678.90f64);
    assert_comparison(1, &DataType::Float64, 678.90f64, 123.45f64);
    assert_comparison(0, &DataType::Float64, 123.45f64, 123.45f64);
}

#[test]
fn test_compare_float4() {
    assert_comparison(-1, &DataType::Float32, 123.45f32, 678.90f32);
    assert_comparison(1, &DataType::Float32, 678.90f32, 123.45f32);
    assert_comparison(0, &DataType::Float32, 123.45f32, 123.45f32);
}

#[test]
fn test_compare_varchar() {
    assert_comparison(-23, &DataType::Utf8, STRING_ABC, STRING_XYZ);
    assert_comparison(23, &DataType::Utf8, STRING_XYZ, STRING_ABC);
    assert_comparison(0, &DataType::Utf8, STRING_ABC, STRING_ABC);
}

#[test]
fn test_compare_varbinary() {
    assert_comparison(-1, &DataType::Binary, binary1(), binary2());
    assert_comparison(1, &DataType::Binary, binary2(), binary1());
    assert_comparison(0, &DataType::Binary, binary1(), binary1());
}

#[test]
fn test_compare_decimal() {
    let t = DataType::Decimal128(10, 2);
    assert_comparison(-1, &t, decimal1(), decimal2());
    assert_comparison(1, &t, decimal2(), decimal1());
    assert_comparison(0, &t, decimal1(), decimal1());
}

#[test]
fn test_compare_bit() {
    assert_comparison(1, &DataType::Boolean, true, false);
    assert_comparison(-1, &DataType::Boolean, false, true);
    assert_comparison(0, &DataType::Boolean, true, true);
}

#[test]
fn test_compare_date_milli() {
    assert_comparison(-1, &date_milli_type(), date1(), date2());
    assert_comparison(1, &date_milli_type(), date2(), date1());
    assert_comparison(0, &date_milli_type(), date1(), date1());
}

#[test]
fn test_compare_date_day() {
    assert_comparison(-1, &DataType::Date32, 1, 2);
    assert_comparison(1, &DataType::Date32, 2, 1);
    assert_comparison(0, &DataType::Date32, 1, 1);
}

#[test]
fn test_compare_struct_equal() {
    let struct2 = Value::struct_of([("key1", Value::from("value1")), ("key2", Value::Int(42))]);
    assert_struct_comparison(0, &struct1(), &struct2);
}

#[test]
fn test_compare_struct_not_equal() {
    assert_struct_comparison(1, &struct1(), &struct2());
}

#[test]
fn test_compare_struct_with_null() {
    assert_struct_comparison(-1, &struct1(), &Value::Null);
    assert_struct_comparison(1, &Value::Null, &struct1());
}

#[test]
fn test_compare_struct_both_null() {
    assert_struct_comparison(0, &Value::Null, &Value::Null);
}

fn connector_message(err: FederationError) -> String {
    match err {
        FederationError::AthenaConnector(e) => e.message().to_string(),
        other => panic!("expected AthenaConnectorException but got {other:?}"),
    }
}

#[test]
fn test_compare_unsupported_struct_type() {
    let err =
        ArrowTypeComparator::compare(&unsupported_type(), &struct1(), &struct2()).unwrap_err();
    assert_eq!(
        "Unknown type INTERVALYEAR object: class java.util.Map",
        connector_message(err)
    );
}

#[test]
fn test_unsupported_type() {
    let err = ArrowTypeComparator::compare(
        &unsupported_type(),
        &Value::from("lhs"),
        &Value::from("rhs"),
    )
    .unwrap_err();
    assert_eq!(
        "Unknown type INTERVALYEAR object: class java.lang.String",
        connector_message(err)
    );
}

#[test]
fn test_compare_nulls() {
    assert_comparison(0, &int_type(), Value::Null, Value::Null);
    assert_comparison(1, &int_type(), Value::Null, 10);
    assert_comparison(-1, &int_type(), 10, Value::Null);
}
