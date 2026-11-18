use std::fmt;

use arrow_schema::{DataType, TimeUnit};
use athena_federation_sdk::data::java_format;
use bigdecimal::BigDecimal;
use chrono::{DateTime, FixedOffset};
use num_bigint::{BigInt, Sign};

use crate::error::{Result, SubstraitError};
use crate::java_cast::{int_to_byte, int_to_short, long_to_int};
use crate::proto::expression::literal::{Decimal, LiteralType};
use crate::proto::expression::{Literal, RexType};
use crate::proto::Expression;

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Boolean(bool),
    Bytes(Vec<u8>),
    Decimal(BigDecimal),
    ZonedDateTime(DateTime<FixedOffset>),
}

impl LiteralValue {
    pub fn java_class_name(&self) -> &'static str {
        match self {
            LiteralValue::Int(_) => "java.lang.Integer",
            LiteralValue::Long(_) => "java.lang.Long",
            LiteralValue::Float(_) => "java.lang.Float",
            LiteralValue::Double(_) => "java.lang.Double",
            LiteralValue::String(_) => "java.lang.String",
            LiteralValue::Boolean(_) => "java.lang.Boolean",
            LiteralValue::Bytes(_) => "[B",
            LiteralValue::Decimal(_) => "java.math.BigDecimal",
            LiteralValue::ZonedDateTime(_) => "java.time.ZonedDateTime",
        }
    }
}

impl fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiteralValue::Int(v) => write!(f, "{v}"),
            LiteralValue::Long(v) => write!(f, "{v}"),
            LiteralValue::Float(v) => f.write_str(&java_format::float_to_string(*v)),
            LiteralValue::Double(v) => f.write_str(&java_format::double_to_string(*v)),
            LiteralValue::String(v) => f.write_str(v),
            LiteralValue::Boolean(v) => write!(f, "{v}"),
            LiteralValue::Bytes(v) => write!(f, "[B@{}", java_format::bytes_to_hex(v)),
            LiteralValue::Decimal(v) => f.write_str(&java_format::big_decimal_to_string(v)),
            LiteralValue::ZonedDateTime(v) => f.write_str(&v.to_rfc3339()),
        }
    }
}

const DECIMAL_BYTE_WIDTH: usize = 16;

#[allow(
    deprecated,
    reason = "reads deprecated Substrait proto fields that the Java converter still reads"
)]
pub fn extract(expr: &Expression) -> Result<(LiteralValue, DataType)> {
    let Some(RexType::Literal(literal)) = &expr.rex_type else {
        return Err(SubstraitError::illegal_argument(
            "Expected literal value in expression",
        ));
    };
    let Some(literal_type) = &literal.literal_type else {
        return Err(SubstraitError::unsupported(
            "Unsupported literal type: LITERALTYPE_NOT_SET",
        ));
    };
    match literal_type {
        LiteralType::I8(v) => Ok((LiteralValue::Int(*v), DataType::Int8)),
        LiteralType::I16(v) => Ok((LiteralValue::Int(*v), DataType::Int16)),
        LiteralType::I32(v) => Ok((LiteralValue::Int(*v), DataType::Int32)),
        LiteralType::I64(v) => Ok((LiteralValue::Long(*v), DataType::Int64)),
        LiteralType::Fp32(v) => Ok((LiteralValue::Float(*v), DataType::Float32)),
        LiteralType::Fp64(v) => Ok((LiteralValue::Double(*v), DataType::Float64)),
        LiteralType::Boolean(v) => Ok((LiteralValue::Boolean(*v), DataType::Boolean)),
        LiteralType::Binary(v) => Ok((LiteralValue::Bytes(v.clone()), DataType::Binary)),
        LiteralType::Decimal(d) => {
            let precision = u8::try_from(d.precision).map_err(|_| {
                SubstraitError::illegal_argument(format!(
                    "Invalid decimal precision: {}",
                    d.precision
                ))
            })?;
            let scale = i8::try_from(d.scale).map_err(|_| {
                SubstraitError::illegal_argument(format!("Invalid decimal scale: {}", d.scale))
            })?;
            let value = big_decimal_from_bytes(&d.value, i64::from(d.scale));
            Ok((
                LiteralValue::Decimal(value),
                DataType::Decimal128(precision, scale),
            ))
        }
        LiteralType::Date(v) => Ok((LiteralValue::Long(i64::from(*v)), DataType::Date32)),
        LiteralType::Timestamp(v) => Ok((
            LiteralValue::Long(*v),
            DataType::Timestamp(TimeUnit::Microsecond, None),
        )),
        LiteralType::VarChar(v) => Ok((LiteralValue::String(v.value.clone()), DataType::Utf8)),
        LiteralType::String(v) | LiteralType::FixedChar(v) => {
            Ok((LiteralValue::String(v.clone()), DataType::Utf8))
        }
        other => Err(SubstraitError::unsupported(format!(
            "Unsupported literal type: {}",
            literal_type_case_name(other)
        ))),
    }
}

pub fn to_expression(value: &LiteralValue, arrow_type: &DataType) -> Result<Expression> {
    let literal_type = match arrow_type {
        DataType::Utf8 => match value {
            LiteralValue::String(s) => LiteralType::String(s.clone()),
            other => return Err(class_cast(other, "java.lang.String")),
        },
        DataType::Binary => match value {
            LiteralValue::Bytes(b) => LiteralType::Binary(b.clone()),
            other => return Err(class_cast(other, "[B")),
        },
        DataType::Decimal32(_, _)
        | DataType::Decimal64(_, _)
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => match value {
            LiteralValue::Decimal(d) => decimal_literal(d)?,
            other => return Err(class_cast(other, "java.math.BigDecimal")),
        },
        DataType::Date32 | DataType::Date64 => match value {
            LiteralValue::Long(v) => LiteralType::Date(long_to_int(*v)),
            other => return Err(class_cast(other, "java.lang.Long")),
        },
        DataType::Int8 | DataType::UInt8 => match value {
            LiteralValue::Int(v) => LiteralType::I8(i32::from(int_to_byte(*v))),
            other => return Err(class_cast(other, "java.lang.Integer")),
        },
        DataType::Int16 | DataType::UInt16 => match value {
            LiteralValue::Int(v) => LiteralType::I16(i32::from(int_to_short(*v))),
            other => return Err(class_cast(other, "java.lang.Integer")),
        },
        DataType::Int32 | DataType::UInt32 => match value {
            LiteralValue::Int(v) => LiteralType::I32(*v),
            other => return Err(class_cast(other, "java.lang.Integer")),
        },
        DataType::Int64 | DataType::UInt64 => match value {
            LiteralValue::Long(v) => LiteralType::I64(*v),
            other => return Err(class_cast(other, "java.lang.Long")),
        },
        DataType::Boolean => match value {
            LiteralValue::Boolean(v) => LiteralType::Boolean(*v),
            other => return Err(class_cast(other, "java.lang.Boolean")),
        },
        DataType::Float64 => match value {
            LiteralValue::Double(v) => LiteralType::Fp64(*v),
            other => return Err(class_cast(other, "java.lang.Double")),
        },
        DataType::Float16 | DataType::Float32 => match value {
            LiteralValue::Float(v) => LiteralType::Fp32(*v),
            other => return Err(class_cast(other, "java.lang.Float")),
        },
        #[allow(
            deprecated,
            reason = "reads deprecated Substrait proto fields that the Java converter still reads"
        )]
        DataType::Timestamp(_, Some(_)) => match value {
            LiteralValue::ZonedDateTime(v) => {
                LiteralType::TimestampTz(i64::from(v.timestamp_subsec_micros()))
            }
            other => return Err(class_cast(other, "java.time.ZonedDateTime")),
        },
        #[allow(
            deprecated,
            reason = "reads deprecated Substrait proto fields that the Java converter still reads"
        )]
        DataType::Timestamp(_, None) => match value {
            LiteralValue::Long(v) => LiteralType::Timestamp(*v),
            other => return Err(class_cast(other, "java.lang.Long")),
        },
        other => {
            return Err(SubstraitError::illegal_argument(format!(
                "Unsupported arrow type: {other}"
            )))
        }
    };
    Ok(Expression {
        rex_type: Some(RexType::Literal(Literal {
            nullable: true,
            type_variation_reference: 0,
            literal_type: Some(literal_type),
        })),
    })
}

fn class_cast(value: &LiteralValue, target: &str) -> SubstraitError {
    SubstraitError::class_cast(value.java_class_name(), target)
}

pub fn big_decimal_from_bytes(value: &[u8], scale: i64) -> BigDecimal {
    BigDecimal::new(BigInt::from_signed_bytes_le(value), scale)
}

fn decimal_literal(value: &BigDecimal) -> Result<LiteralType> {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    let precision = if unscaled.sign() == Sign::NoSign {
        1
    } else {
        unscaled.magnitude().to_string().len()
    };
    let mut bytes = unscaled.to_signed_bytes_le();
    if bytes.len() > DECIMAL_BYTE_WIDTH {
        return Err(SubstraitError::illegal_argument(format!(
            "Decimal value {} does not fit in {} bytes",
            java_format::big_decimal_to_string(value),
            DECIMAL_BYTE_WIDTH
        )));
    }
    let fill = if unscaled.sign() == Sign::Minus {
        0xFF
    } else {
        0x00
    };
    bytes.resize(DECIMAL_BYTE_WIDTH, fill);
    let scale = i32::try_from(scale)
        .map_err(|_| SubstraitError::illegal_argument(format!("Invalid decimal scale: {scale}")))?;
    let precision = i32::try_from(precision)
        .map_err(|_| SubstraitError::illegal_argument("Invalid decimal precision"))?;
    Ok(LiteralType::Decimal(Decimal {
        value: bytes,
        precision,
        scale,
    }))
}

#[allow(
    deprecated,
    reason = "reads deprecated Substrait proto fields that the Java converter still reads"
)]
pub fn literal_type_case_name(literal_type: &LiteralType) -> &'static str {
    match literal_type {
        LiteralType::Boolean(_) => "BOOLEAN",
        LiteralType::I8(_) => "I8",
        LiteralType::I16(_) => "I16",
        LiteralType::I32(_) => "I32",
        LiteralType::I64(_) => "I64",
        LiteralType::Fp32(_) => "FP32",
        LiteralType::Fp64(_) => "FP64",
        LiteralType::String(_) => "STRING",
        LiteralType::Binary(_) => "BINARY",
        LiteralType::Timestamp(_) => "TIMESTAMP",
        LiteralType::Date(_) => "DATE",
        LiteralType::Time(_) => "TIME",
        LiteralType::IntervalYearToMonth(_) => "INTERVAL_YEAR_TO_MONTH",
        LiteralType::IntervalDayToSecond(_) => "INTERVAL_DAY_TO_SECOND",
        LiteralType::IntervalCompound(_) => "INTERVAL_COMPOUND",
        LiteralType::FixedChar(_) => "FIXED_CHAR",
        LiteralType::VarChar(_) => "VAR_CHAR",
        LiteralType::FixedBinary(_) => "FIXED_BINARY",
        LiteralType::Decimal(_) => "DECIMAL",
        LiteralType::PrecisionTime(_) => "PRECISION_TIME",
        LiteralType::PrecisionTimestamp(_) => "PRECISION_TIMESTAMP",
        LiteralType::PrecisionTimestampTz(_) => "PRECISION_TIMESTAMP_TZ",
        LiteralType::Struct(_) => "STRUCT",
        LiteralType::Map(_) => "MAP",
        LiteralType::TimestampTz(_) => "TIMESTAMP_TZ",
        LiteralType::Uuid(_) => "UUID",
        LiteralType::Null(_) => "NULL",
        LiteralType::List(_) => "LIST",
        LiteralType::EmptyList(_) => "EMPTY_LIST",
        LiteralType::EmptyMap(_) => "EMPTY_MAP",
        LiteralType::UserDefined(_) => "USER_DEFINED",
    }
}
