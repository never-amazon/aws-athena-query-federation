use std::cmp::Ordering;

use super::date_time_formatter_util::DateTimeFormatterUtil;
use super::types::{minor_type_of, ArrowType, MinorType};
use super::value::{java_double_compare, java_float_compare, Value};
use super::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub struct ArrowTypeComparator;

fn ord(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

pub fn java_string_compare(a: &str, b: &str) -> i32 {
    let mut ai = a.encode_utf16();
    let mut bi = b.encode_utf16();
    let remaining = |n: usize| i32::try_from(1 + n).unwrap_or(i32::MAX);
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) if x != y => return i32::from(x) - i32::from(y),
            (Some(_), Some(_)) => {}
            (Some(_), None) => return remaining(ai.count()),
            (None, Some(_)) => return -remaining(bi.count()),
            (None, None) => return 0,
        }
    }
}

pub fn compare_unsigned_bytes(a: &[u8], b: &[u8]) -> i32 {
    for (x, y) in a.iter().zip(b.iter()) {
        if x != y {
            return if x < y { -1 } else { 1 };
        }
    }
    ord(a.len().cmp(&b.len()))
}

fn cast_err(type_name: MinorType, lhs: &Value, rhs: &Value) -> FederationError {
    FederationError::ClassCast(format!(
        "cannot compare {} and {} as {}",
        lhs.java_class_name(),
        rhs.java_class_name(),
        type_name
    ))
}

impl ArrowTypeComparator {
    pub fn compare_reader(reader: &FieldReader<'_>, lhs: &Value, rhs: &Value) -> Result<i32> {
        ArrowTypeComparator::compare(reader.field().data_type(), lhs, rhs)
    }

    pub fn compare(arrow_type: &ArrowType, lhs: &Value, rhs: &Value) -> Result<i32> {
        match (lhs.is_null(), rhs.is_null()) {
            (true, true) => return Ok(0),
            (true, false) => return Ok(1),
            (false, true) => return Ok(-1),
            _ => {}
        }
        let minor = minor_type_of(arrow_type);
        match (minor, lhs, rhs) {
            (
                MinorType::INT | MinorType::UINT4 | MinorType::DATEDAY,
                Value::Int(a),
                Value::Int(b),
            ) => Ok(ord(a.cmp(b))),
            (MinorType::TINYINT | MinorType::UINT1, Value::Byte(a), Value::Byte(b)) => {
                Ok(i32::from(*a) - i32::from(*b))
            }
            (MinorType::SMALLINT, Value::Short(a), Value::Short(b)) => {
                Ok(i32::from(*a) - i32::from(*b))
            }
            (MinorType::UINT2, Value::Char(a), Value::Char(b)) => Ok(i32::from(*a) - i32::from(*b)),
            (MinorType::BIGINT | MinorType::UINT8, Value::Long(a), Value::Long(b)) => {
                Ok(ord(a.cmp(b)))
            }
            (MinorType::FLOAT8, Value::Double(a), Value::Double(b)) => {
                Ok(ord(java_double_compare(*a, *b)))
            }
            (MinorType::FLOAT4, Value::Float(a), Value::Float(b)) => {
                Ok(ord(java_float_compare(*a, *b)))
            }
            (MinorType::VARCHAR, _, _) => Ok(java_string_compare(
                &lhs.java_to_string(),
                &rhs.java_to_string(),
            )),
            (MinorType::VARBINARY, Value::Bytes(a), Value::Bytes(b)) => {
                Ok(compare_unsigned_bytes(a, b))
            }
            (MinorType::DECIMAL, Value::Decimal(a), Value::Decimal(b)) => Ok(ord(a.cmp(b))),
            (MinorType::BIT, Value::Boolean(a), Value::Boolean(b)) => Ok(ord(a.cmp(b))),
            (
                MinorType::DATEMILLI | MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ,
                Value::LocalDateTime(a),
                Value::LocalDateTime(b),
            ) => Ok(ord(a.cmp(b))),
            (
                MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ,
                Value::Long(a),
                Value::Long(b),
            ) => {
                let l = DateTimeFormatterUtil::construct_zoned_date_time(*a, arrow_type)?;
                let r = DateTimeFormatterUtil::construct_zoned_date_time(*b, arrow_type)?;
                Ok(ord(l.compare_to(&r)))
            }
            (
                MinorType::INT
                | MinorType::UINT4
                | MinorType::DATEDAY
                | MinorType::TINYINT
                | MinorType::UINT1
                | MinorType::SMALLINT
                | MinorType::UINT2
                | MinorType::BIGINT
                | MinorType::UINT8
                | MinorType::FLOAT8
                | MinorType::FLOAT4
                | MinorType::VARBINARY
                | MinorType::DECIMAL
                | MinorType::BIT
                | MinorType::DATEMILLI
                | MinorType::TIMESTAMPMICROTZ
                | MinorType::TIMESTAMPMILLITZ,
                _,
                _,
            ) => Err(cast_err(minor, lhs, rhs)),
            (MinorType::MAP | MinorType::LIST | MinorType::STRUCT, _, _) => {
                if lhs == rhs {
                    Ok(0)
                } else if lhs.java_hash_code() < rhs.java_hash_code() {
                    Ok(-1)
                } else {
                    Ok(1)
                }
            }
            (other, _, _) => {
                tracing::warn!(
                    "compare: Unknown type {} object: {}",
                    other,
                    lhs.java_class_name()
                );
                Err(FederationError::invalid_input(format!(
                    "Unknown type {} object: class {}",
                    other,
                    lhs.java_class_name()
                )))
            }
        }
    }
}
