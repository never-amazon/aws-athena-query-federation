use std::str::FromStr;

use athena_federation_sdk::data::java_format::big_decimal_to_string;
use athena_jdbc::sql::SqlValue;
use bigdecimal::BigDecimal;
use chrono::{DateTime, NaiveDate, NaiveDateTime};

use super::types::{oid, TypeCache};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f";
const TIMESTAMPTZ_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f%#z";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decoder {
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Date,
    Timestamp,
    TimestampTz,
    Bytea,
    Array {
        element: Box<Decoder>,
        delimiter: u8,
    },
    Text,
}

impl Decoder {
    pub(crate) fn for_type(types: &TypeCache, type_oid: u32) -> Decoder {
        match type_oid {
            oid::BOOL => Decoder::Bool,
            oid::INT2 => Decoder::Int2,
            oid::INT4 => Decoder::Int4,
            oid::INT8 | oid::OID => Decoder::Int8,
            oid::FLOAT4 => Decoder::Float4,
            oid::FLOAT8 => Decoder::Float8,
            oid::NUMERIC => Decoder::Numeric,
            oid::DATE => Decoder::Date,
            oid::TIMESTAMP => Decoder::Timestamp,
            oid::TIMESTAMPTZ => Decoder::TimestampTz,
            oid::BYTEA => Decoder::Bytea,
            other => match types
                .get(other)
                .and_then(|t| t.element.map(|e| (e, t.delimiter)))
            {
                Some((element, delimiter)) => Decoder::Array {
                    element: Box::new(Decoder::for_type(types, element)),
                    delimiter,
                },
                None => Decoder::Text,
            },
        }
    }

    pub(crate) fn decode(&self, raw: Option<Vec<u8>>) -> SqlValue {
        let Some(raw) = raw else {
            return SqlValue::Null;
        };
        let text = match String::from_utf8(raw) {
            Ok(text) => text,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        };
        self.decode_text(text)
    }

    fn decode_text(&self, text: String) -> SqlValue {
        match self {
            Decoder::Text => SqlValue::String(text),
            Decoder::Bool => match text.as_str() {
                "t" => SqlValue::rendered(SqlValue::Boolean(true), text),
                "f" => SqlValue::rendered(SqlValue::Boolean(false), text),
                _ => SqlValue::String(text),
            },
            Decoder::Int2 => text
                .parse()
                .map_or(SqlValue::String(text), SqlValue::SmallInt),
            Decoder::Int4 => text
                .parse()
                .map_or(SqlValue::String(text), SqlValue::Integer),
            Decoder::Int8 => text
                .parse()
                .map_or(SqlValue::String(text), SqlValue::BigInt),
            Decoder::Float4 => match parse_f32(&text) {
                Some(v) => SqlValue::rendered(SqlValue::Real(v), text),
                None => SqlValue::String(text),
            },
            Decoder::Float8 => match parse_f64(&text) {
                Some(v) => SqlValue::rendered(SqlValue::Double(v), text),
                None => SqlValue::String(text),
            },
            Decoder::Numeric => match BigDecimal::from_str(&text) {
                Ok(d) if big_decimal_to_string(&d) == text => SqlValue::Decimal(d),
                Ok(d) => SqlValue::rendered(SqlValue::Decimal(d), text),
                Err(_) => SqlValue::String(text),
            },
            Decoder::Date => NaiveDate::parse_from_str(&text, "%Y-%m-%d")
                .map_or(SqlValue::String(text), SqlValue::Date),
            Decoder::Timestamp => match NaiveDateTime::parse_from_str(&text, TIMESTAMP_FORMAT) {
                Ok(ts) => SqlValue::rendered(SqlValue::Timestamp(ts), text),
                Err(_) => SqlValue::String(text),
            },
            Decoder::TimestampTz => match DateTime::parse_from_str(&text, TIMESTAMPTZ_FORMAT) {
                Ok(ts) => SqlValue::rendered(SqlValue::TimestampTz(ts), text),
                Err(_) => SqlValue::String(text),
            },
            Decoder::Bytea => decode_bytea(&text).map_or(SqlValue::String(text), SqlValue::Bytes),
            Decoder::Array { element, delimiter } => match parse_array(&text, *delimiter) {
                Some(items) => {
                    SqlValue::rendered(SqlValue::Array(element.decode_items(items)), text)
                }
                None => SqlValue::String(text),
            },
        }
    }

    fn decode_items(&self, items: Vec<ArrayItem>) -> Vec<SqlValue> {
        items
            .into_iter()
            .map(|item| match item {
                ArrayItem::Null => SqlValue::Null,
                ArrayItem::Value(text) => self.decode_element(text),
                ArrayItem::Nested(nested) => SqlValue::Array(self.decode_items(nested)),
            })
            .collect()
    }

    fn decode_element(&self, text: String) -> SqlValue {
        match self.decode_text(text) {
            SqlValue::Rendered { value, .. } => *value,
            other => other,
        }
    }
}

fn parse_f32(text: &str) -> Option<f32> {
    match text {
        "Infinity" => Some(f32::INFINITY),
        "-Infinity" => Some(f32::NEG_INFINITY),
        "NaN" => Some(f32::NAN),
        other => other.parse().ok(),
    }
}

fn parse_f64(text: &str) -> Option<f64> {
    match text {
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        other => other.parse().ok(),
    }
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn decode_bytea(text: &str) -> Option<Vec<u8>> {
    if let Some(hex) = text.strip_prefix("\\x") {
        let bytes = hex.as_bytes();
        if bytes.len() % 2 != 0 {
            return None;
        }
        return bytes
            .chunks(2)
            .map(|pair| Some(hex_digit(pair[0])? * 16 + hex_digit(pair[1])?))
            .collect();
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes.get(i + 1) == Some(&b'\\') {
                out.push(b'\\');
                i += 2;
                continue;
            }
            let octal = bytes.get(i + 1..i + 4)?;
            let mut value: u32 = 0;
            for digit in octal {
                if !(b'0'..=b'7').contains(digit) {
                    return None;
                }
                value = value * 8 + u32::from(digit - b'0');
            }
            out.push(u8::try_from(value).ok()?);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArrayItem {
    Null,
    Value(String),
    Nested(Vec<ArrayItem>),
}

pub(crate) fn parse_array(text: &str, delimiter: u8) -> Option<Vec<ArrayItem>> {
    let start = if text.starts_with('[') {
        text.find('=')? + 1
    } else {
        0
    };
    let bytes = text.as_bytes();
    let mut pos = start;
    let items = parse_level(bytes, &mut pos, delimiter)?;
    (pos == bytes.len()).then_some(items)
}

fn parse_level(bytes: &[u8], pos: &mut usize, delimiter: u8) -> Option<Vec<ArrayItem>> {
    if bytes.get(*pos) != Some(&b'{') {
        return None;
    }
    *pos += 1;
    let mut items = Vec::new();
    if bytes.get(*pos) == Some(&b'}') {
        *pos += 1;
        return Some(items);
    }
    loop {
        match bytes.get(*pos)? {
            b'{' => items.push(ArrayItem::Nested(parse_level(bytes, pos, delimiter)?)),
            b'"' => {
                *pos += 1;
                let mut value = Vec::new();
                loop {
                    match bytes.get(*pos)? {
                        b'\\' => {
                            value.push(*bytes.get(*pos + 1)?);
                            *pos += 2;
                        }
                        b'"' => {
                            *pos += 1;
                            break;
                        }
                        other => {
                            value.push(*other);
                            *pos += 1;
                        }
                    }
                }
                items.push(ArrayItem::Value(String::from_utf8(value).ok()?));
            }
            _ => {
                let begin = *pos;
                while let Some(b) = bytes.get(*pos) {
                    if *b == delimiter || *b == b'}' {
                        break;
                    }
                    *pos += 1;
                }
                let raw = std::str::from_utf8(&bytes[begin..*pos]).ok()?.trim();
                if raw.eq_ignore_ascii_case("NULL") {
                    items.push(ArrayItem::Null);
                } else {
                    items.push(ArrayItem::Value(raw.to_string()));
                }
            }
        }
        match bytes.get(*pos)? {
            b'}' => {
                *pos += 1;
                return Some(items);
            }
            b if *b == delimiter => *pos += 1,
            _ => return None,
        }
    }
}

fn format_f32(value: f32) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else {
        value.to_string()
    }
}

fn format_f64(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string()
    } else {
        value.to_string()
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("\\x");
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
    }
    out
}

pub(crate) fn encode_parameter(
    value: &SqlValue,
    string_unspecified: bool,
) -> (u32, Option<String>) {
    match value {
        SqlValue::Null => (0, None),
        SqlValue::Boolean(b) => (
            oid::BOOL,
            Some(if *b { "TRUE" } else { "FALSE" }.to_string()),
        ),
        SqlValue::TinyInt(v) => (oid::INT2, Some(v.to_string())),
        SqlValue::SmallInt(v) => (oid::INT2, Some(v.to_string())),
        SqlValue::Integer(v) => (oid::INT4, Some(v.to_string())),
        SqlValue::BigInt(v) => (oid::INT8, Some(v.to_string())),
        SqlValue::Real(v) => (oid::FLOAT4, Some(format_f32(*v))),
        SqlValue::Double(v) => (oid::FLOAT8, Some(format_f64(*v))),
        SqlValue::Decimal(d) => (oid::NUMERIC, Some(d.to_plain_string())),
        SqlValue::String(s) => (
            if string_unspecified { 0 } else { oid::VARCHAR },
            Some(s.clone()),
        ),
        SqlValue::Bytes(b) => (oid::BYTEA, Some(hex_bytes(b))),
        SqlValue::Date(d) => (0, Some(format!("{} +00", d.format("%Y-%m-%d")))),
        SqlValue::Time(t) => (0, Some(t.format("%H:%M:%S%.f").to_string())),
        SqlValue::Timestamp(ts) => (0, Some(format!("{}+00", ts.format(TIMESTAMP_FORMAT)))),
        SqlValue::TimestampTz(ts) => (
            0,
            Some(format!("{}+00", ts.naive_utc().format(TIMESTAMP_FORMAT))),
        ),
        SqlValue::Array(_) => (0, Some(value.to_string())),
        SqlValue::Rendered { value, .. } => encode_parameter(value, string_unspecified),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athena_jdbc::sql::Types;

    fn cache() -> TypeCache {
        TypeCache::default()
    }

    #[test]
    fn booleans_keep_the_server_text() {
        let value = Decoder::Bool.decode(Some(b"t".to_vec()));
        assert_eq!(value.to_boolean().unwrap(), Some(true));
        assert_eq!(value.to_java_string().as_deref(), Some("t"));
    }

    #[test]
    fn timestamps_with_time_zone_parse_short_offsets() {
        let value = Decoder::TimestampTz.decode(Some(b"2000-01-02 12:59:00+00".to_vec()));
        assert_eq!(
            value.to_timestamp().unwrap().unwrap().to_string(),
            "2000-01-02 12:59:00"
        );
        assert_eq!(
            value.to_java_string().as_deref(),
            Some("2000-01-02 12:59:00+00")
        );
        let shifted = Decoder::TimestampTz.decode(Some(b"2000-01-01 05:30:00+05:30".to_vec()));
        assert_eq!(
            shifted.to_timestamp().unwrap().unwrap().to_string(),
            "2000-01-01 00:00:00"
        );
    }

    #[test]
    fn arrays_parse_quotes_nulls_and_nesting() {
        let items = parse_array(r#"{"a b",NULL,"c\"d","e\\f","NULL",""}"#, b',').unwrap();
        assert_eq!(
            items,
            vec![
                ArrayItem::Value("a b".to_string()),
                ArrayItem::Null,
                ArrayItem::Value("c\"d".to_string()),
                ArrayItem::Value("e\\f".to_string()),
                ArrayItem::Value("NULL".to_string()),
                ArrayItem::Value(String::new()),
            ]
        );
        let nested = parse_array("{{1,2},{3,4}}", b',').unwrap();
        assert_eq!(nested.len(), 2);
        assert_eq!(parse_array("[2:3]={1,2}", b',').unwrap().len(), 2);
        assert_eq!(parse_array("{}", b',').unwrap(), Vec::new());
    }

    #[test]
    fn int_arrays_decode_to_typed_elements_and_keep_text() {
        let decoder = Decoder::for_type(&cache(), 1007);
        let value = decoder.decode(Some(b"{7,8,9}".to_vec()));
        assert_eq!(
            value.to_array().unwrap().unwrap(),
            vec![
                SqlValue::Integer(7),
                SqlValue::Integer(8),
                SqlValue::Integer(9)
            ]
        );
        assert_eq!(value.to_java_string().as_deref(), Some("{7,8,9}"));
        assert_eq!(
            value.to_long().unwrap_err().message(),
            "Bad value for type long : {7,8,9}"
        );
    }

    #[test]
    fn bytea_hex_and_escape_formats_decode() {
        assert_eq!(decode_bytea("\\x0000000a").unwrap(), vec![0, 0, 0, 10]);
        assert_eq!(
            decode_bytea("ab\\\\\\001").unwrap(),
            vec![b'a', b'b', b'\\', 1]
        );
    }

    #[test]
    fn numeric_keeps_plain_text_when_java_would_use_an_exponent() {
        let value = Decoder::Numeric.decode(Some(b"0.0000001".to_vec()));
        assert_eq!(value.to_java_string().as_deref(), Some("0.0000001"));
        assert_eq!(
            value.to_big_decimal().unwrap().unwrap(),
            BigDecimal::from_str("0.0000001").unwrap()
        );
        let plain = Decoder::Numeric.decode(Some(b"86.4199".to_vec()));
        assert!(matches!(plain, SqlValue::Decimal(_)));
        assert_eq!(plain.to_long().unwrap(), Some(86));
    }

    #[test]
    fn parameters_use_pgjdbc_type_oids() {
        assert_eq!(encode_parameter(&SqlValue::Real(0.1), false).0, oid::FLOAT4);
        assert_eq!(
            encode_parameter(&SqlValue::Double(0.1), false).0,
            oid::FLOAT8
        );
        assert_eq!(
            encode_parameter(&SqlValue::String("x".into()), false),
            (oid::VARCHAR, Some("x".to_string()))
        );
        assert_eq!(encode_parameter(&SqlValue::String("x".into()), true).0, 0);
        assert_eq!(
            encode_parameter(
                &SqlValue::Date(NaiveDate::from_ymd_opt(2020, 1, 2).unwrap()),
                false
            ),
            (0, Some("2020-01-02 +00".to_string()))
        );
        assert_eq!(
            encode_parameter(&SqlValue::Boolean(true), false),
            (oid::BOOL, Some("TRUE".to_string()))
        );
        assert_eq!(Types::ARRAY, cache().sql_type(1007));
    }
}
