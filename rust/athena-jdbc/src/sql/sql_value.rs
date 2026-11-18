use std::fmt;
use std::str::FromStr;

use athena_federation_sdk::data::java_format::{
    big_decimal_to_string, double_to_string, float_to_string,
};
use athena_federation_sdk::data::{local_date_to_string, Value};
use bigdecimal::BigDecimal;
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use num_traits::{FromPrimitive, ToPrimitive, Zero};

use super::sql_exception::{SqlException, SQL_EXCEPTION_CLASS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderedKind {
    Typed,
    JavaDateMillis(i64),
    InvalidElement(&'static str),
    InvalidDateTime,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Boolean(bool),
    TinyInt(i8),
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Real(f32),
    Double(f64),
    Decimal(BigDecimal),
    String(String),
    Bytes(Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    Timestamp(NaiveDateTime),
    TimestampTz(DateTime<FixedOffset>),
    Array(Vec<SqlValue>),
    Rendered {
        value: Box<SqlValue>,
        text: String,
        exception_class: &'static str,
        kind: RenderedKind,
    },
}

fn bad_value(type_name: &str, value: &SqlValue) -> SqlException {
    let class_name = match value {
        SqlValue::Rendered {
            exception_class, ..
        } => exception_class,
        _ => SQL_EXCEPTION_CLASS,
    };
    SqlException::with_class_name(
        class_name,
        format!("Bad value for type {type_name} : {value}"),
    )
}

fn invalid_date_time(value: &SqlValue) -> Option<SqlException> {
    match value {
        SqlValue::Rendered {
            text,
            exception_class,
            kind: RenderedKind::InvalidDateTime,
            ..
        } => Some(SqlException::with_class_name(
            exception_class,
            format!("Bad value for type timestamp/date/time: {text}"),
        )),
        _ => None,
    }
}

fn first_invalid_element(items: &[SqlValue]) -> Option<&SqlValue> {
    items.iter().find_map(|item| match item {
        SqlValue::Rendered {
            kind: RenderedKind::InvalidElement(_),
            ..
        } => Some(item),
        SqlValue::Array(nested) => first_invalid_element(nested),
        SqlValue::Rendered { value, .. } => match value.as_ref() {
            SqlValue::Array(nested) => first_invalid_element(nested),
            _ => None,
        },
        _ => None,
    })
}

fn truncate_decimal(value: &BigDecimal) -> BigDecimal {
    value.with_scale_round(0, bigdecimal::RoundingMode::Down)
}

fn timestamp_to_string(ts: &NaiveDateTime) -> String {
    let base = ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let nanos = ts.nanosecond() % 1_000_000_000;
    if nanos == 0 {
        return format!("{base}.0");
    }
    let digits = format!("{nanos:09}");
    format!("{base}.{}", digits.trim_end_matches('0'))
}

fn parse_timestamp(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ]
    .iter()
    .find_map(|format| NaiveDateTime::parse_from_str(text, format).ok())
    .or_else(|| {
        NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
    })
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    let text = text.trim();
    let date_part = text.get(..10).unwrap_or(text);
    NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()
}

impl SqlValue {
    pub fn rendered(value: SqlValue, text: impl Into<String>) -> SqlValue {
        SqlValue::Rendered {
            value: Box::new(value),
            text: text.into(),
            exception_class: SQL_EXCEPTION_CLASS,
            kind: RenderedKind::Typed,
        }
    }

    pub fn java_date_millis(millis: i64, text: impl Into<String>) -> SqlValue {
        let text = text.into();
        SqlValue::Rendered {
            value: Box::new(SqlValue::String(text.clone())),
            text,
            exception_class: SQL_EXCEPTION_CLASS,
            kind: RenderedKind::JavaDateMillis(millis),
        }
    }

    pub fn invalid_element(java_type: &'static str, text: impl Into<String>) -> SqlValue {
        let text = text.into();
        SqlValue::Rendered {
            value: Box::new(SqlValue::String(text.clone())),
            text,
            exception_class: SQL_EXCEPTION_CLASS,
            kind: RenderedKind::InvalidElement(java_type),
        }
    }

    pub fn invalid_date_time(text: impl Into<String>) -> SqlValue {
        let text = text.into();
        SqlValue::Rendered {
            value: Box::new(SqlValue::String(text.clone())),
            text,
            exception_class: SQL_EXCEPTION_CLASS,
            kind: RenderedKind::InvalidDateTime,
        }
    }

    #[must_use]
    pub fn with_exception_class(self, class_name: &'static str) -> SqlValue {
        match self {
            SqlValue::Rendered {
                value, text, kind, ..
            } => SqlValue::Rendered {
                value,
                text,
                exception_class: class_name,
                kind,
            },
            other => other,
        }
    }

    pub fn to_timestamp_millis(&self) -> Result<Option<i64>, SqlException> {
        if let Some(error) = invalid_date_time(self) {
            return Err(error);
        }
        if let SqlValue::Rendered {
            kind: RenderedKind::JavaDateMillis(millis),
            ..
        } = self
        {
            return Ok(Some(*millis));
        }
        Ok(self
            .to_timestamp()?
            .map(|ts| ts.and_utc().timestamp_millis()))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, SqlValue::Null)
    }

    fn to_integral(&self, type_name: &str) -> Result<Option<BigDecimal>, SqlException> {
        let decimal = match self {
            SqlValue::Null => return Ok(None),
            SqlValue::Boolean(b) => BigDecimal::from(i32::from(*b)),
            SqlValue::TinyInt(v) => BigDecimal::from(*v),
            SqlValue::SmallInt(v) => BigDecimal::from(*v),
            SqlValue::Integer(v) => BigDecimal::from(*v),
            SqlValue::BigInt(v) => BigDecimal::from(*v),
            SqlValue::Real(v) => {
                BigDecimal::from_f32(v.trunc()).ok_or_else(|| bad_value(type_name, self))?
            }
            SqlValue::Double(v) => {
                BigDecimal::from_f64(v.trunc()).ok_or_else(|| bad_value(type_name, self))?
            }
            SqlValue::Decimal(d) => truncate_decimal(d),
            SqlValue::String(s) => BigDecimal::from_str(s.trim())
                .map(|d| truncate_decimal(&d))
                .map_err(|_| bad_value(type_name, self))?,
            _ => return Err(bad_value(type_name, self)),
        };
        Ok(Some(decimal))
    }

    pub fn to_boolean(&self) -> Result<Option<bool>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_boolean().map_err(|_| bad_value("boolean", self));
        }
        match self {
            SqlValue::Null => Ok(None),
            SqlValue::Boolean(b) => Ok(Some(*b)),
            SqlValue::String(s) => match s.trim().to_lowercase().as_str() {
                "1" | "true" | "t" | "yes" | "y" | "on" => Ok(Some(true)),
                "0" | "false" | "f" | "no" | "n" | "off" => Ok(Some(false)),
                _ => Err(bad_value("boolean", self)),
            },
            other => match other.to_integral("boolean") {
                Ok(Some(d)) => Ok(Some(!d.is_zero())),
                Ok(None) => Ok(None),
                Err(_) => Err(bad_value("boolean", self)),
            },
        }
    }

    pub fn to_byte(&self) -> Result<Option<i8>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_byte().map_err(|_| bad_value("byte", self));
        }
        self.to_integral("byte")?
            .map(|d| d.to_i8().ok_or_else(|| bad_value("byte", self)))
            .transpose()
    }

    pub fn to_short(&self) -> Result<Option<i16>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_short().map_err(|_| bad_value("short", self));
        }
        self.to_integral("short")?
            .map(|d| d.to_i16().ok_or_else(|| bad_value("short", self)))
            .transpose()
    }

    pub fn to_int(&self) -> Result<Option<i32>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_int().map_err(|_| bad_value("int", self));
        }
        self.to_integral("int")?
            .map(|d| d.to_i32().ok_or_else(|| bad_value("int", self)))
            .transpose()
    }

    pub fn to_long(&self) -> Result<Option<i64>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_long().map_err(|_| bad_value("long", self));
        }
        self.to_integral("long")?
            .map(|d| d.to_i64().ok_or_else(|| bad_value("long", self)))
            .transpose()
    }

    pub fn to_double(&self) -> Result<Option<f64>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_double().map_err(|_| bad_value("double", self));
        }
        let value = match self {
            SqlValue::Null => return Ok(None),
            SqlValue::Boolean(b) => f64::from(u8::from(*b)),
            SqlValue::TinyInt(v) => f64::from(*v),
            SqlValue::SmallInt(v) => f64::from(*v),
            SqlValue::Integer(v) => f64::from(*v),
            SqlValue::BigInt(v) => v.to_f64().ok_or_else(|| bad_value("double", self))?,
            SqlValue::Real(v) => f64::from(*v),
            SqlValue::Double(v) => *v,
            SqlValue::Decimal(d) => d.to_f64().ok_or_else(|| bad_value("double", self))?,
            SqlValue::String(s) => s
                .trim()
                .parse::<f64>()
                .map_err(|_| bad_value("double", self))?,
            _ => return Err(bad_value("double", self)),
        };
        Ok(Some(value))
    }

    pub fn to_float(&self) -> Result<Option<f32>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_float().map_err(|_| bad_value("float", self));
        }
        match self {
            SqlValue::Real(v) => Ok(Some(*v)),
            SqlValue::String(s) => s
                .trim()
                .parse::<f32>()
                .map(Some)
                .map_err(|_| bad_value("float", self)),
            other => Ok(other.to_double()?.and_then(|d| d.to_f32())),
        }
    }

    pub fn to_big_decimal(&self) -> Result<Option<BigDecimal>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value
                .to_big_decimal()
                .map_err(|_| bad_value("BigDecimal", self));
        }
        let value = match self {
            SqlValue::Null => return Ok(None),
            SqlValue::Boolean(b) => BigDecimal::from(i32::from(*b)),
            SqlValue::TinyInt(v) => BigDecimal::from(*v),
            SqlValue::SmallInt(v) => BigDecimal::from(*v),
            SqlValue::Integer(v) => BigDecimal::from(*v),
            SqlValue::BigInt(v) => BigDecimal::from(*v),
            SqlValue::Real(v) => BigDecimal::from_str(&float_to_string(*v))
                .map_err(|_| bad_value("BigDecimal", self))?,
            SqlValue::Double(v) => BigDecimal::from_str(&double_to_string(*v))
                .map_err(|_| bad_value("BigDecimal", self))?,
            SqlValue::Decimal(d) => d.clone(),
            SqlValue::String(s) => {
                BigDecimal::from_str(s.trim()).map_err(|_| bad_value("BigDecimal", self))?
            }
            _ => return Err(bad_value("BigDecimal", self)),
        };
        Ok(Some(value))
    }

    pub fn to_java_string(&self) -> Option<String> {
        match self {
            SqlValue::Null => None,
            other => Some(other.to_string()),
        }
    }

    pub fn to_bytes(&self) -> Result<Option<Vec<u8>>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_bytes().map_err(|_| bad_value("bytes", self));
        }
        match self {
            SqlValue::Null => Ok(None),
            SqlValue::Bytes(b) => Ok(Some(b.clone())),
            SqlValue::String(s) => Ok(Some(s.as_bytes().to_vec())),
            other => Err(bad_value("bytes", other)),
        }
    }

    pub fn to_date(&self) -> Result<Option<NaiveDate>, SqlException> {
        if let Some(error) = invalid_date_time(self) {
            return Err(error);
        }
        if let SqlValue::Rendered { value, .. } = self {
            return value.to_date().map_err(|_| bad_value("date", self));
        }
        match self {
            SqlValue::Null => Ok(None),
            SqlValue::Date(d) => Ok(Some(*d)),
            SqlValue::Timestamp(ts) => Ok(Some(ts.date())),
            SqlValue::TimestampTz(ts) => Ok(Some(ts.naive_utc().date())),
            SqlValue::String(s) => parse_date(s)
                .map(Some)
                .ok_or_else(|| bad_value("date", self)),
            other => Err(bad_value("date", other)),
        }
    }

    pub fn to_timestamp(&self) -> Result<Option<NaiveDateTime>, SqlException> {
        if let Some(error) = invalid_date_time(self) {
            return Err(error);
        }
        if let SqlValue::Rendered { value, .. } = self {
            return value
                .to_timestamp()
                .map_err(|_| bad_value("timestamp", self));
        }
        match self {
            SqlValue::Null => Ok(None),
            SqlValue::Timestamp(ts) => Ok(Some(*ts)),
            SqlValue::TimestampTz(ts) => Ok(Some(ts.naive_utc())),
            SqlValue::Date(d) => Ok(d.and_hms_opt(0, 0, 0)),
            SqlValue::String(s) => parse_timestamp(s)
                .map(Some)
                .ok_or_else(|| bad_value("timestamp", self)),
            other => Err(bad_value("timestamp", other)),
        }
    }

    pub fn to_array(&self) -> Result<Option<Vec<SqlValue>>, SqlException> {
        if let SqlValue::Rendered { value, .. } = self {
            let items = value.to_array().map_err(|_| bad_value("array", self))?;
            if let Some(
                invalid @ SqlValue::Rendered {
                    kind: RenderedKind::InvalidElement(java_type),
                    ..
                },
            ) = items.as_deref().and_then(first_invalid_element)
            {
                return Err(bad_value(java_type, invalid));
            }
            return Ok(items);
        }
        match self {
            SqlValue::Null => Ok(None),
            SqlValue::Array(items) => Ok(Some(items.clone())),
            other => Err(bad_value("array", other)),
        }
    }

    pub fn to_value(&self) -> Value {
        match self {
            SqlValue::Null => Value::Null,
            SqlValue::Boolean(b) => Value::Boolean(*b),
            SqlValue::TinyInt(v) => Value::Byte(*v),
            SqlValue::SmallInt(v) => Value::Short(*v),
            SqlValue::Integer(v) => Value::Int(*v),
            SqlValue::BigInt(v) => Value::Long(*v),
            SqlValue::Real(v) => Value::Float(*v),
            SqlValue::Double(v) => Value::Double(*v),
            SqlValue::Decimal(d) => Value::Decimal(d.clone()),
            SqlValue::String(s) => Value::String(s.clone()),
            SqlValue::Bytes(b) => Value::Bytes(b.clone()),
            SqlValue::Date(d) => Value::LocalDate(*d),
            SqlValue::Time(_) => Value::String(self.to_string()),
            SqlValue::Timestamp(ts) => Value::Date(ts.and_utc().timestamp_millis()),
            SqlValue::TimestampTz(ts) => Value::Date(ts.timestamp_millis()),
            SqlValue::Array(items) => Value::List(items.iter().map(SqlValue::to_value).collect()),
            SqlValue::Rendered {
                kind: RenderedKind::JavaDateMillis(millis),
                ..
            } => Value::Date(*millis),
            SqlValue::Rendered { value, text, .. } => match value.as_ref() {
                SqlValue::Array(_) => Value::String(text.clone()),
                other => other.to_value(),
            },
        }
    }
}

impl fmt::Display for SqlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SqlValue::Null => f.write_str("null"),
            SqlValue::Boolean(b) => write!(f, "{b}"),
            SqlValue::TinyInt(v) => write!(f, "{v}"),
            SqlValue::SmallInt(v) => write!(f, "{v}"),
            SqlValue::Integer(v) => write!(f, "{v}"),
            SqlValue::BigInt(v) => write!(f, "{v}"),
            SqlValue::Real(v) => f.write_str(&float_to_string(*v)),
            SqlValue::Double(v) => f.write_str(&double_to_string(*v)),
            SqlValue::Decimal(d) => f.write_str(&big_decimal_to_string(d)),
            SqlValue::String(s) => f.write_str(s),
            SqlValue::Bytes(b) => {
                f.write_str("\\x")?;
                b.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
            }
            SqlValue::Date(d) => f.write_str(&local_date_to_string(d)),
            SqlValue::Time(t) => write!(f, "{}", t.format("%H:%M:%S")),
            SqlValue::Timestamp(ts) => f.write_str(&timestamp_to_string(ts)),
            SqlValue::TimestampTz(ts) => f.write_str(&timestamp_to_string(&ts.naive_utc())),
            SqlValue::Array(items) => {
                f.write_str("{")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("}")
            }
            SqlValue::Rendered { text, .. } => f.write_str(text),
        }
    }
}
