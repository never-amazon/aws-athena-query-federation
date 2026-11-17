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
    let mut alen = 0i32;
    let mut blen = 0i32;
    loop {
        match (ai.next(), bi.next()) {
            (Some(x), Some(y)) => {
                alen += 1;
                blen += 1;
                if x != y {
                    return x as i32 - y as i32;
                }
            }
            (Some(_), None) => {
                alen += 1 + ai.count() as i32;
                return alen - blen;
            }
            (None, Some(_)) => {
                blen += 1 + bi.count() as i32;
                return alen - blen;
            }
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
        ArrowTypeComparator::compare(reader.get_field().data_type(), lhs, rhs)
    }

    pub fn compare(arrow_type: &ArrowType, lhs: &Value, rhs: &Value) -> Result<i32> {
        match (lhs.is_null(), rhs.is_null()) {
            (true, true) => return Ok(0),
            (true, false) => return Ok(1),
            (false, true) => return Ok(-1),
            _ => {}
        }
        let t = minor_type_of(arrow_type);
        let err = || cast_err(t, lhs, rhs);
        match t {
            MinorType::INT | MinorType::UINT4 | MinorType::DATEDAY => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::TINYINT | MinorType::UINT1 => match (lhs, rhs) {
                (Value::Byte(a), Value::Byte(b)) => Ok(*a as i32 - *b as i32),
                _ => Err(err()),
            },
            MinorType::SMALLINT => match (lhs, rhs) {
                (Value::Short(a), Value::Short(b)) => Ok(*a as i32 - *b as i32),
                _ => Err(err()),
            },
            MinorType::UINT2 => match (lhs, rhs) {
                (Value::Char(a), Value::Char(b)) => Ok(*a as i32 - *b as i32),
                _ => Err(err()),
            },
            MinorType::BIGINT | MinorType::UINT8 => match (lhs, rhs) {
                (Value::Long(a), Value::Long(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::FLOAT8 => match (lhs, rhs) {
                (Value::Double(a), Value::Double(b)) => Ok(ord(java_double_compare(*a, *b))),
                _ => Err(err()),
            },
            MinorType::FLOAT4 => match (lhs, rhs) {
                (Value::Float(a), Value::Float(b)) => Ok(ord(java_float_compare(*a, *b))),
                _ => Err(err()),
            },
            MinorType::VARCHAR => Ok(java_string_compare(&lhs.java_to_string(), &rhs.java_to_string())),
            MinorType::VARBINARY => match (lhs, rhs) {
                (Value::Bytes(a), Value::Bytes(b)) => Ok(compare_unsigned_bytes(a, b)),
                _ => Err(err()),
            },
            MinorType::DECIMAL => match (lhs, rhs) {
                (Value::Decimal(a), Value::Decimal(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::BIT => match (lhs, rhs) {
                (Value::Boolean(a), Value::Boolean(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::DATEMILLI => match (lhs, rhs) {
                (Value::LocalDateTime(a), Value::LocalDateTime(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ => match (lhs, rhs) {
                (Value::Long(a), Value::Long(b)) => {
                    let l = DateTimeFormatterUtil::construct_zoned_date_time(*a, arrow_type)?;
                    let r = DateTimeFormatterUtil::construct_zoned_date_time(*b, arrow_type)?;
                    Ok(ord(l.compare_to(&r)))
                }
                (Value::Long(_), _) => Err(err()),
                (Value::LocalDateTime(a), Value::LocalDateTime(b)) => Ok(ord(a.cmp(b))),
                _ => Err(err()),
            },
            MinorType::MAP | MinorType::LIST | MinorType::STRUCT => {
                if lhs == rhs {
                    Ok(0)
                } else if lhs.java_hash_code() < rhs.java_hash_code() {
                    Ok(-1)
                } else {
                    Ok(1)
                }
            }
            other => {
                tracing::warn!("compare: Unknown type {} object: {}", other, lhs.java_class_name());
                Err(FederationError::invalid_input(format!(
                    "Unknown type {} object: class {}",
                    other,
                    lhs.java_class_name()
                )))
            }
        }
    }
}
