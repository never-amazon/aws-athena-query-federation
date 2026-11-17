use std::fmt;

use bigdecimal::BigDecimal;

use crate::calcite::sql_type_name::SqlTypeName;
use crate::calcite::values::{
    java_big_decimal_to_string, java_double_to_string, java_float_to_string, BitString, DateString,
    TimeString, TimestampString,
};
use crate::error::{Result, SubstraitError};
use crate::java_cast::{double_to_long, long_to_double};

#[derive(Debug, Clone, PartialEq)]
pub enum SubstraitValue {
    Null,
    Boolean(bool),
    Byte(i8),
    Short(i16),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Decimal(BigDecimal),
    String(String),
    Bytes(Vec<u8>),
    Date(DateString),
    Time(TimeString),
    Timestamp(TimestampString),
    Bits(BitString),
    Symbol(String),
}

impl SubstraitValue {
    pub fn is_null(&self) -> bool {
        matches!(self, SubstraitValue::Null)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            SubstraitValue::Byte(v) => Some(i64::from(*v)),
            SubstraitValue::Short(v) => Some(i64::from(*v)),
            SubstraitValue::Integer(v) => Some(i64::from(*v)),
            SubstraitValue::Long(v) => Some(*v),
            SubstraitValue::Float(v) => Some(double_to_long(f64::from(*v))),
            SubstraitValue::Double(v) => Some(double_to_long(*v)),
            SubstraitValue::Decimal(d) => {
                let (unscaled, scale) = d.with_scale(0).as_bigint_and_exponent();
                if scale != 0 {
                    return None;
                }
                let bytes = unscaled.to_signed_bytes_le();
                let mut buf = [if unscaled.sign() == num_bigint::Sign::Minus {
                    0xFF
                } else {
                    0
                }; 8];
                for (i, b) in bytes.iter().take(8).enumerate() {
                    buf[i] = *b;
                }
                Some(i64::from_le_bytes(buf))
            }
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            SubstraitValue::Float(v) => Some(f64::from(*v)),
            SubstraitValue::Double(v) => Some(*v),
            SubstraitValue::Decimal(d) => java_big_decimal_to_string(d).parse().ok(),
            other => other.as_i64().map(long_to_double),
        }
    }

    pub fn as_decimal(&self) -> Option<&BigDecimal> {
        match self {
            SubstraitValue::Decimal(d) => Some(d),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SubstraitValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn java_class_name(&self) -> &'static str {
        match self {
            SubstraitValue::Null => "null",
            SubstraitValue::Boolean(_) => "java.lang.Boolean",
            SubstraitValue::Byte(_) => "java.lang.Byte",
            SubstraitValue::Short(_) => "java.lang.Short",
            SubstraitValue::Integer(_) => "java.lang.Integer",
            SubstraitValue::Long(_) => "java.lang.Long",
            SubstraitValue::Float(_) => "java.lang.Float",
            SubstraitValue::Double(_) => "java.lang.Double",
            SubstraitValue::Decimal(_) => "java.math.BigDecimal",
            SubstraitValue::String(_) => "java.lang.String",
            SubstraitValue::Bytes(_) => "[B",
            SubstraitValue::Date(_) => "org.apache.calcite.util.DateString",
            SubstraitValue::Time(_) => "org.apache.calcite.util.TimeString",
            SubstraitValue::Timestamp(_) => "org.apache.calcite.util.TimestampString",
            SubstraitValue::Bits(_) => "org.apache.calcite.util.BitString",
            SubstraitValue::Symbol(_) => "java.lang.Enum",
        }
    }
}

impl fmt::Display for SubstraitValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubstraitValue::Null => f.write_str("null"),
            SubstraitValue::Boolean(b) => write!(f, "{b}"),
            SubstraitValue::Byte(v) => write!(f, "{v}"),
            SubstraitValue::Short(v) => write!(f, "{v}"),
            SubstraitValue::Integer(v) => write!(f, "{v}"),
            SubstraitValue::Long(v) => write!(f, "{v}"),
            SubstraitValue::Float(v) => f.write_str(&java_float_to_string(*v)),
            SubstraitValue::Double(v) => f.write_str(&java_double_to_string(*v)),
            SubstraitValue::Decimal(d) => f.write_str(&java_big_decimal_to_string(d)),
            SubstraitValue::String(s) | SubstraitValue::Symbol(s) => f.write_str(s),
            SubstraitValue::Bytes(b) => write!(f, "{b:?}"),
            SubstraitValue::Date(d) => write!(f, "{d}"),
            SubstraitValue::Time(t) => write!(f, "{t}"),
            SubstraitValue::Timestamp(t) => write!(f, "{t}"),
            SubstraitValue::Bits(b) => write!(f, "{b}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubstraitTypeAndValue {
    type_name: SqlTypeName,
    value: SubstraitValue,
    column_name: String,
}

#[derive(Debug, Default)]
pub struct SubstraitTypeAndValueBuilder {
    type_name: Option<SqlTypeName>,
    value: Option<SubstraitValue>,
    column_name: Option<String>,
}

impl SubstraitTypeAndValueBuilder {
    #[must_use]
    pub fn type_name(mut self, type_name: SqlTypeName) -> Self {
        self.type_name = Some(type_name);
        self
    }

    #[must_use]
    pub fn value(mut self, value: SubstraitValue) -> Self {
        self.value = Some(value);
        self
    }

    #[must_use]
    pub fn column_name(mut self, column_name: impl Into<String>) -> Self {
        self.column_name = Some(column_name.into());
        self
    }

    pub fn build(self) -> Result<SubstraitTypeAndValue> {
        let type_name = self
            .type_name
            .ok_or_else(|| SubstraitError::null_pointer("type is null"))?;
        let value = self
            .value
            .filter(|v| !v.is_null())
            .ok_or_else(|| SubstraitError::null_pointer("value is null"))?;
        let column_name = self
            .column_name
            .ok_or_else(|| SubstraitError::null_pointer("value is null"))?;
        Ok(SubstraitTypeAndValue {
            type_name,
            value,
            column_name,
        })
    }
}

impl SubstraitTypeAndValue {
    pub fn new(
        type_name: SqlTypeName,
        value: SubstraitValue,
        column_name: impl Into<String>,
    ) -> Result<SubstraitTypeAndValue> {
        SubstraitTypeAndValue::builder()
            .type_name(type_name)
            .value(value)
            .column_name(column_name)
            .build()
    }

    pub fn builder() -> SubstraitTypeAndValueBuilder {
        SubstraitTypeAndValueBuilder::default()
    }

    pub fn get_type(&self) -> SqlTypeName {
        self.type_name
    }

    pub fn value(&self) -> &SubstraitValue {
        &self.value
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }
}

impl fmt::Display for SubstraitTypeAndValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TypeAndValue{{type={}, value={}}}",
            self.type_name, self.value
        )
    }
}
