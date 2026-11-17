use std::any::Any;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use num_bigint::BigInt;

use super::java_format::{big_decimal_to_string, double_to_string, float_to_string, java_string_hash};
use super::java_time::{local_date_time_to_string, local_date_to_string, ZonedDateTime};

#[derive(Clone)]
pub struct OpaqueValue(pub Arc<dyn Any + Send + Sync>);

impl OpaqueValue {
    pub fn new<T: Any + Send + Sync>(value: T) -> OpaqueValue {
        OpaqueValue(Arc::new(value))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl fmt::Debug for OpaqueValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpaqueValue")
    }
}

#[derive(Debug, Clone, Default)]
pub enum Value {
    #[default]
    Null,
    Boolean(bool),
    Byte(i8),
    Short(i16),
    Char(u16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    BigInteger(BigInt),
    Decimal(BigDecimal),
    String(String),
    Bytes(Vec<u8>),
    Date(i64),
    Instant(DateTime<Utc>),
    LocalDate(NaiveDate),
    LocalDateTime(NaiveDateTime),
    ZonedDateTime(ZonedDateTime),
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Opaque(OpaqueValue),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_number(&self) -> bool {
        matches!(
            self,
            Value::Byte(_)
                | Value::Short(_)
                | Value::Int(_)
                | Value::Long(_)
                | Value::Float(_)
                | Value::Double(_)
                | Value::BigInteger(_)
                | Value::Decimal(_)
        )
    }

    pub fn java_class_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Boolean(_) => "java.lang.Boolean",
            Value::Byte(_) => "java.lang.Byte",
            Value::Short(_) => "java.lang.Short",
            Value::Char(_) => "java.lang.Character",
            Value::Int(_) => "java.lang.Integer",
            Value::Long(_) => "java.lang.Long",
            Value::Float(_) => "java.lang.Float",
            Value::Double(_) => "java.lang.Double",
            Value::BigInteger(_) => "java.math.BigInteger",
            Value::Decimal(_) => "java.math.BigDecimal",
            Value::String(_) => "java.lang.String",
            Value::Bytes(_) => "[B",
            Value::Date(_) => "java.util.Date",
            Value::Instant(_) => "java.time.Instant",
            Value::LocalDate(_) => "java.time.LocalDate",
            Value::LocalDateTime(_) => "java.time.LocalDateTime",
            Value::ZonedDateTime(_) => "java.time.ZonedDateTime",
            Value::List(_) => "java.util.List",
            Value::Map(_) => "java.util.Map",
            Value::Opaque(_) => "java.lang.Object",
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(v) => Some(v.as_str()),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&[(Value, Value)]> {
        match self {
            Value::Map(v) => Some(v.as_slice()),
            _ => None,
        }
    }

    pub fn map_get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Value::String(s) if s == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn struct_of<K: Into<String>>(entries: impl IntoIterator<Item = (K, Value)>) -> Value {
        Value::Map(
            entries
                .into_iter()
                .map(|(k, v)| (Value::String(k.into()), v))
                .collect(),
        )
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Byte(v) => Some(*v as f64),
            Value::Short(v) => Some(*v as f64),
            Value::Int(v) => Some(*v as f64),
            Value::Long(v) => Some(*v as f64),
            Value::Float(v) => Some(*v as f64),
            Value::Double(v) => Some(*v),
            Value::BigInteger(v) => num_traits::ToPrimitive::to_f64(v),
            Value::Decimal(v) => num_traits::ToPrimitive::to_f64(v),
            _ => None,
        }
    }

    pub fn number_long_value(&self) -> Option<i64> {
        use num_traits::ToPrimitive;
        match self {
            Value::Byte(v) => Some(*v as i64),
            Value::Short(v) => Some(*v as i64),
            Value::Int(v) => Some(*v as i64),
            Value::Long(v) => Some(*v),
            Value::Float(v) => Some(java_f64_to_long(*v as f64)),
            Value::Double(v) => Some(java_f64_to_long(*v)),
            Value::BigInteger(v) => Some(low_i64_of_bigint(v)),
            Value::Decimal(v) => {
                let (int_part, _) = v.with_scale(0).into_bigint_and_exponent();
                Some(int_part.to_i64().unwrap_or_else(|| low_i64_of_bigint(&int_part)))
            }
            _ => None,
        }
    }

    pub fn number_int_value(&self) -> Option<i32> {
        match self {
            Value::Float(v) => Some(java_f64_to_int(*v as f64)),
            Value::Double(v) => Some(java_f64_to_int(*v)),
            other => other.number_long_value().map(|v| v as i32),
        }
    }

    pub fn java_hash_code(&self) -> i32 {
        match self {
            Value::Null => 0,
            Value::Boolean(b) => {
                if *b {
                    1231
                } else {
                    1237
                }
            }
            Value::Byte(v) => *v as i32,
            Value::Short(v) => *v as i32,
            Value::Char(v) => *v as i32,
            Value::Int(v) => *v,
            Value::Long(v) => (*v ^ ((*v as u64) >> 32) as i64) as i32,
            Value::Float(v) => float_bits(*v) as i32,
            Value::Double(v) => {
                let bits = double_bits(*v);
                (bits ^ (bits >> 32)) as i32
            }
            Value::BigInteger(v) => bigint_hash(v),
            Value::Decimal(v) => {
                let (unscaled, scale) = v.as_bigint_and_exponent();
                31i32.wrapping_mul(bigint_hash(&unscaled)).wrapping_add(scale as i32)
            }
            Value::String(s) => java_string_hash(s),
            Value::Bytes(b) => {
                let mut h: i32 = 1;
                for x in b {
                    h = h.wrapping_mul(31).wrapping_add(*x as i8 as i32);
                }
                h
            }
            Value::Date(ms) => (*ms ^ ((*ms as u64) >> 32) as i64) as i32,
            Value::Instant(i) => {
                let s = i.timestamp();
                ((s ^ ((s as u64) >> 32) as i64) as i32).wrapping_add(51i32.wrapping_mul(i.timestamp_subsec_nanos() as i32))
            }
            Value::LocalDate(d) => {
                use chrono::Datelike;
                let y = d.year();
                let m = d.month() as i32;
                let dd = d.day() as i32;
                (y & 0xFFFFF800u32 as i32) ^ ((y << 11) + (m << 6) + dd)
            }
            Value::LocalDateTime(dt) => {
                use chrono::Timelike;
                let date_hash = Value::LocalDate(dt.date()).java_hash_code();
                let nod = dt.num_seconds_from_midnight() as i64 * 1_000_000_000 + (dt.nanosecond() % 1_000_000_000) as i64;
                date_hash ^ ((nod ^ ((nod as u64) >> 32) as i64) as i32)
            }
            Value::ZonedDateTime(z) => {
                Value::LocalDateTime(z.to_local_date_time()).java_hash_code()
                    ^ z.get_offset().local_minus_utc()
                    ^ java_string_hash(z.get_zone().get_id()).rotate_left(3)
            }
            Value::List(items) => {
                let mut h: i32 = 1;
                for x in items {
                    h = h.wrapping_mul(31).wrapping_add(x.java_hash_code());
                }
                h
            }
            Value::Map(entries) => entries
                .iter()
                .fold(0i32, |acc, (k, v)| acc.wrapping_add(k.java_hash_code() ^ v.java_hash_code())),
            Value::Opaque(o) => Arc::as_ptr(&o.0) as *const () as usize as i32,
        }
    }

    pub fn java_to_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::Byte(v) => v.to_string(),
            Value::Short(v) => v.to_string(),
            Value::Char(v) => char::from_u32(*v as u32).map(|c| c.to_string()).unwrap_or_default(),
            Value::Int(v) => v.to_string(),
            Value::Long(v) => v.to_string(),
            Value::Float(v) => float_to_string(*v),
            Value::Double(v) => double_to_string(*v),
            Value::BigInteger(v) => v.to_string(),
            Value::Decimal(v) => big_decimal_to_string(v),
            Value::String(s) => s.clone(),
            Value::Bytes(b) => format!("[B@{:x}", b.as_ptr() as usize),
            Value::Date(ms) => format!("Date({})", ms),
            Value::Instant(i) => i.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true),
            Value::LocalDate(d) => local_date_to_string(d),
            Value::LocalDateTime(d) => local_date_time_to_string(d),
            Value::ZonedDateTime(z) => z.to_string(),
            Value::List(items) => {
                let parts: Vec<String> = items.iter().map(|v| v.java_to_string()).collect();
                format!("[{}]", parts.join(", "))
            }
            Value::Map(entries) => {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| format!("{}={}", k.java_to_string(), v.java_to_string()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
            Value::Opaque(_) => "java.lang.Object".to_string(),
        }
    }
}

fn java_f64_to_long(v: f64) -> i64 {
    if v.is_nan() {
        0
    } else {
        v as i64
    }
}

fn java_f64_to_int(v: f64) -> i32 {
    if v.is_nan() {
        0
    } else {
        v as i32
    }
}

fn low_i64_of_bigint(v: &BigInt) -> i64 {
    let (sign, digits) = v.to_u64_digits();
    let low = digits.first().copied().unwrap_or(0);
    if sign == num_bigint::Sign::Minus {
        (low as i64).wrapping_neg()
    } else {
        low as i64
    }
}

fn bigint_hash(v: &BigInt) -> i32 {
    let (sign, digits) = v.to_u32_digits();
    let mut h: i32 = 0;
    for d in digits.iter().rev() {
        h = h.wrapping_mul(31).wrapping_add(*d as i32);
    }
    match sign {
        num_bigint::Sign::Minus => h.wrapping_neg(),
        num_bigint::Sign::NoSign => 0,
        num_bigint::Sign::Plus => h,
    }
}

fn float_bits(v: f32) -> u32 {
    if v.is_nan() {
        0x7fc0_0000
    } else {
        v.to_bits()
    }
}

fn double_bits(v: f64) -> u64 {
    if v.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        v.to_bits()
    }
}

pub fn java_float_compare(a: f32, b: f32) -> Ordering {
    if a < b {
        return Ordering::Less;
    }
    if a > b {
        return Ordering::Greater;
    }
    let ab = float_bits(a) as i32;
    let bb = float_bits(b) as i32;
    ab.cmp(&bb)
}

pub fn java_double_compare(a: f64, b: f64) -> Ordering {
    if a < b {
        return Ordering::Less;
    }
    if a > b {
        return Ordering::Greater;
    }
    let ab = double_bits(a) as i64;
    let bb = double_bits(b) as i64;
    ab.cmp(&bb)
}

pub fn decimal_equals(a: &BigDecimal, b: &BigDecimal) -> bool {
    let (au, ascale) = a.as_bigint_and_exponent();
    let (bu, bscale) = b.as_bigint_and_exponent();
    ascale == bscale && au == bu
}

fn map_equals(a: &[(Value, Value)], b: &[(Value, Value)]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .all(|(k, v)| b.iter().any(|(k2, v2)| k == k2 && v == v2))
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Byte(a), Value::Byte(b)) => a == b,
            (Value::Short(a), Value::Short(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Long(a), Value::Long(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => float_bits(*a) == float_bits(*b),
            (Value::Double(a), Value::Double(b)) => double_bits(*a) == double_bits(*b),
            (Value::BigInteger(a), Value::BigInteger(b)) => a == b,
            (Value::Decimal(a), Value::Decimal(b)) => decimal_equals(a, b),
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Bytes(a), Value::Bytes(b)) => a == b,
            (Value::Date(a), Value::Date(b)) => a == b,
            (Value::Instant(a), Value::Instant(b)) => a == b,
            (Value::LocalDate(a), Value::LocalDate(b)) => a == b,
            (Value::LocalDateTime(a), Value::LocalDateTime(b)) => a == b,
            (Value::ZonedDateTime(a), Value::ZonedDateTime(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => map_equals(a, b),
            (Value::Opaque(a), Value::Opaque(b)) => Arc::ptr_eq(&a.0, &b.0),
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.java_to_string())
    }
}

macro_rules! value_from {
    ($t:ty, $variant:ident) => {
        impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::$variant(v)
            }
        }
    };
}

value_from!(bool, Boolean);
value_from!(i8, Byte);
value_from!(i16, Short);
value_from!(i32, Int);
value_from!(i64, Long);
value_from!(f32, Float);
value_from!(f64, Double);
value_from!(BigInt, BigInteger);
value_from!(BigDecimal, Decimal);
value_from!(String, String);
value_from!(Vec<u8>, Bytes);
value_from!(DateTime<Utc>, Instant);
value_from!(NaiveDate, LocalDate);
value_from!(NaiveDateTime, LocalDateTime);
value_from!(ZonedDateTime, ZonedDateTime);
value_from!(Vec<Value>, List);

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_string())
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::String(v.clone())
    }
}

impl From<&[u8]> for Value {
    fn from(v: &[u8]) -> Self {
        Value::Bytes(v.to_vec())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(x) => x.into(),
            None => Value::Null,
        }
    }
}

impl From<&Value> for Value {
    fn from(v: &Value) -> Self {
        v.clone()
    }
}
