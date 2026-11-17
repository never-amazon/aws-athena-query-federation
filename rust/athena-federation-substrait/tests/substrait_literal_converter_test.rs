#![allow(clippy::approx_constant)]

use arrow_schema::{DataType, TimeUnit};
use athena_federation_substrait::proto::expression::literal::{Decimal, LiteralType, VarChar};
use athena_federation_substrait::proto::expression::{Literal, RexType};
use athena_federation_substrait::proto::Expression;
use athena_federation_substrait::{
    LiteralValue, SubstraitLiteralConverter,
};
use athena_federation_substrait::ErrorKind;
use bigdecimal::BigDecimal;
use chrono::{FixedOffset, TimeZone};
use std::str::FromStr;

fn literal_expr(literal_type: Option<LiteralType>) -> Expression {
    Expression {
        rex_type: Some(RexType::Literal(Literal {
            literal_type,
            ..Default::default()
        })),
    }
}

fn extract(literal_type: LiteralType) -> (LiteralValue, DataType) {
    SubstraitLiteralConverter::extract_literal_value(&literal_expr(Some(literal_type))).unwrap()
}

fn created_literal(value: LiteralValue, arrow_type: DataType) -> LiteralType {
    let expr = SubstraitLiteralConverter::create_literal_expression(&value, &arrow_type).unwrap();
    match expr.rex_type {
        Some(RexType::Literal(l)) => {
            assert!(l.nullable);
            l.literal_type.expect("literal type")
        }
        other => panic!("expected literal, got {other:?}"),
    }
}

#[test]
fn test_extract_string_literal() {
    let (value, arrow_type) = extract(LiteralType::String("test".to_string()));
    assert_eq!(LiteralValue::String("test".to_string()), value);
    assert_eq!(DataType::Utf8, arrow_type);
}

#[test]
fn test_extract_integer_literals() {
    let (value, arrow_type) = extract(LiteralType::I32(42));
    assert_eq!(LiteralValue::Int(42), value);
    assert_eq!(DataType::Int32, arrow_type);
}

#[test]
fn test_extract_boolean_literal() {
    let (value, arrow_type) = extract(LiteralType::Boolean(true));
    assert_eq!(LiteralValue::Boolean(true), value);
    assert_eq!(DataType::Boolean, arrow_type);
}

#[test]
fn test_extract_floating_point_literals() {
    let (value, arrow_type) = extract(LiteralType::Fp64(3.14));
    match value {
        LiteralValue::Double(d) => assert!((d - 3.14).abs() < 0.001),
        other => panic!("expected double, got {other:?}"),
    }
    assert_eq!(DataType::Float64, arrow_type);
}

#[test]
fn test_extract_fp32_literal() {
    let (value, arrow_type) = extract(LiteralType::Fp32(2.5));
    match value {
        LiteralValue::Float(f) => assert!((f - 2.5).abs() < 0.001),
        other => panic!("expected float, got {other:?}"),
    }
    assert_eq!(DataType::Float32, arrow_type);
}

#[test]
fn test_create_string_literal() {
    assert_eq!(
        LiteralType::String("hello".to_string()),
        created_literal(LiteralValue::String("hello".to_string()), DataType::Utf8)
    );
}

#[test]
fn test_create_integer_literal() {
    assert_eq!(
        LiteralType::I32(123),
        created_literal(LiteralValue::Int(123), DataType::Int32)
    );
}

#[test]
fn test_create_boolean_literal() {
    assert_eq!(
        LiteralType::Boolean(false),
        created_literal(LiteralValue::Boolean(false), DataType::Boolean)
    );
}

#[test]
fn test_create_decimal_literal() {
    let value = BigDecimal::from_str("123.45").unwrap();
    let literal = created_literal(LiteralValue::Decimal(value), DataType::Decimal128(10, 2));
    let mut bytes = vec![0u8; 16];
    bytes[0] = 0x39;
    bytes[1] = 0x30;
    assert_eq!(
        LiteralType::Decimal(Decimal {
            value: bytes,
            precision: 5,
            scale: 2,
        }),
        literal
    );
}

#[test]
fn test_extract_non_literal_expression() {
    let err = SubstraitLiteralConverter::extract_literal_value(&Expression::default()).unwrap_err();
    assert_eq!(ErrorKind::IllegalArgument, err.kind());
}

#[test]
fn test_extract_binary_literal() {
    let data = vec![1u8, 2, 3, 4];
    let (value, arrow_type) = extract(LiteralType::Binary(data.clone()));
    assert_eq!(LiteralValue::Bytes(data), value);
    assert_eq!(DataType::Binary, arrow_type);
}

#[test]
fn test_extract_date_literal() {
    let (value, arrow_type) = extract(LiteralType::Date(18628));
    assert_eq!(LiteralValue::Long(18628), value);
    assert_eq!(DataType::Date32, arrow_type);
}

#[test]
#[allow(deprecated)]
fn test_extract_timestamp_literal() {
    let (value, arrow_type) = extract(LiteralType::Timestamp(1609459200000000));
    assert_eq!(LiteralValue::Long(1609459200000000), value);
    assert_eq!(DataType::Timestamp(TimeUnit::Microsecond, None), arrow_type);
}

#[test]
fn test_extract_var_char_literal() {
    let (value, arrow_type) = extract(LiteralType::VarChar(VarChar {
        value: "varchar_test".to_string(),
        length: 20,
    }));
    assert_eq!(LiteralValue::String("varchar_test".to_string()), value);
    assert_eq!(DataType::Utf8, arrow_type);
}

#[test]
fn test_extract_fixed_char_literal() {
    let (value, arrow_type) = extract(LiteralType::FixedChar("fixed".to_string()));
    assert_eq!(LiteralValue::String("fixed".to_string()), value);
    assert_eq!(DataType::Utf8, arrow_type);
}

#[test]
fn test_extract_i8_literal() {
    let (value, arrow_type) = extract(LiteralType::I8(8));
    assert_eq!(LiteralValue::Int(8), value);
    assert_eq!(DataType::Int8, arrow_type);
}

#[test]
fn test_extract_i16_literal() {
    let (value, arrow_type) = extract(LiteralType::I16(16));
    assert_eq!(LiteralValue::Int(16), value);
    assert_eq!(DataType::Int16, arrow_type);
}

#[test]
fn test_extract_i64_literal() {
    let (value, arrow_type) = extract(LiteralType::I64(64));
    assert_eq!(LiteralValue::Long(64), value);
    assert_eq!(DataType::Int64, arrow_type);
}

#[test]
fn test_extract_decimal_literal() {
    let mut decimal_bytes = vec![0u8; 16];
    decimal_bytes[15] = 123;
    let (value, arrow_type) = extract(LiteralType::Decimal(Decimal {
        value: decimal_bytes,
        precision: 10,
        scale: 2,
    }));
    assert!(matches!(value, LiteralValue::Decimal(_)));
    assert_eq!(DataType::Decimal128(10, 2), arrow_type);
}

#[test]
fn test_create_i8_literal() {
    assert_eq!(
        LiteralType::I8(8),
        created_literal(LiteralValue::Int(8), DataType::Int8)
    );
}

#[test]
fn test_create_i16_literal() {
    assert_eq!(
        LiteralType::I16(16),
        created_literal(LiteralValue::Int(16), DataType::Int16)
    );
}

#[test]
fn test_create_i64_literal() {
    assert_eq!(
        LiteralType::I64(64),
        created_literal(LiteralValue::Long(64), DataType::Int64)
    );
}

#[test]
fn test_create_fp32_literal() {
    assert_eq!(
        LiteralType::Fp32(3.14),
        created_literal(LiteralValue::Float(3.14), DataType::Float32)
    );
}

#[test]
fn test_create_fp64_literal() {
    assert_eq!(
        LiteralType::Fp64(3.14),
        created_literal(LiteralValue::Double(3.14), DataType::Float64)
    );
}

#[test]
fn test_create_binary_literal() {
    assert_eq!(
        LiteralType::Binary(vec![1, 2, 3]),
        created_literal(LiteralValue::Bytes(vec![1, 2, 3]), DataType::Binary)
    );
}

#[test]
fn test_create_date_literal() {
    assert_eq!(
        LiteralType::Date(18628),
        created_literal(LiteralValue::Long(18628), DataType::Date32)
    );
}

#[test]
#[allow(deprecated)]
fn test_create_timestamp_literal() {
    assert_eq!(
        LiteralType::Timestamp(1609459200000000),
        created_literal(
            LiteralValue::Long(1609459200000000),
            DataType::Timestamp(TimeUnit::Microsecond, None)
        )
    );
}

#[test]
#[allow(deprecated)]
fn test_create_timestamp_tz_literal() {
    let zoned = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2021, 1, 1, 0, 0, 0)
        .unwrap();
    assert_eq!(
        LiteralType::TimestampTz(0),
        created_literal(
            LiteralValue::ZonedDateTime(zoned),
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into()))
        )
    );
}

#[test]
fn test_create_integer_default_bit_width() {
    assert_eq!(
        LiteralType::I32(42),
        created_literal(LiteralValue::Int(42), DataType::UInt32)
    );
}

#[test]
fn test_create_unsupported_arrow_type() {
    let list_type = DataType::List(std::sync::Arc::new(arrow_schema::Field::new(
        "item",
        DataType::Utf8,
        true,
    )));
    let err = SubstraitLiteralConverter::create_literal_expression(
        &LiteralValue::String("test".to_string()),
        &list_type,
    )
    .unwrap_err();
    assert_eq!(ErrorKind::IllegalArgument, err.kind());
}

#[test]
fn test_extract_unsupported_literal_type() {
    let err = SubstraitLiteralConverter::extract_literal_value(&literal_expr(None)).unwrap_err();
    assert_eq!(ErrorKind::UnsupportedOperation, err.kind());
}
