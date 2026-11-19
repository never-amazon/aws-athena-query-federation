use std::borrow::Cow;
use std::str::FromStr;

use athena_federation_sdk::data::java_format::big_decimal_to_string;
use athena_jdbc::sql::{Driver, SqlError, SqlValue};
use bigdecimal::BigDecimal;
use chrono::{Datelike, NaiveDate, NaiveDateTime, TimeDelta, Timelike};

use super::datetime::{
    java_sql_date_epoch_day, millis_to_naive, parse_backend_timestamp, proleptic_epoch_day,
    DATE_NEGATIVE_INFINITY, DATE_POSITIVE_INFINITY,
};
use super::types::{oid, TypeCache};

const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.f";
const NANOS_PER_MICRO: u32 = 1000;
const MIN_TIMESTAMP_DATE: Option<NaiveDate> = NaiveDate::from_ymd_opt(-4713, 11, 24);
const TIMESTAMP_OUT_OF_RANGE: &str =
    "Timestamp is outside the PostgreSQL range 4714-11-24 00:00:00 BC to 294276-12-31 23:59:59.999999";
pub(crate) const MAX_ARRAY_DEPTH: usize = 6;

fn rendered(value: SqlValue, text: String) -> SqlValue {
    SqlValue::rendered(value, text).with_driver(Driver::Postgres)
}

fn invalid_date_time(text: Cow<'_, str>) -> SqlValue {
    SqlValue::invalid_date_time(text.into_owned()).with_driver(Driver::Postgres)
}

fn infinity_millis(text: &str) -> Option<i64> {
    match text {
        "infinity" => Some(DATE_POSITIVE_INFINITY),
        "-infinity" => Some(DATE_NEGATIVE_INFINITY),
        _ => None,
    }
}

fn java_timestamp(text: &str, zoned: bool) -> Option<SqlValue> {
    if let Some(millis) = infinity_millis(text) {
        return Some(
            SqlValue::java_date_millis(millis, proleptic_epoch_day(millis, zoned), text)
                .with_driver(Driver::Postgres),
        );
    }
    let parsed = parse_backend_timestamp(text)?;
    let millis = parsed.java_millis()?;
    let epoch_millis = parsed.epoch_millis();
    let epoch_day = proleptic_epoch_day(epoch_millis.unwrap_or(millis), zoned);
    Some(
        match (millis_to_naive(millis, parsed.nanos), epoch_millis) {
            (Some(ts), Some(epoch_millis)) => {
                SqlValue::java_timestamp(SqlValue::Timestamp(ts), text, epoch_millis, epoch_day)
                    .with_driver(Driver::Postgres)
            }
            (Some(ts), None) => rendered(SqlValue::Timestamp(ts), text.to_string()),
            (None, _) => {
                SqlValue::java_date_millis(millis, epoch_day, text).with_driver(Driver::Postgres)
            }
        },
    )
}

fn java_date_millis(text: &str) -> Option<i64> {
    infinity_millis(text).or_else(|| parse_backend_timestamp(text)?.java_date_millis())
}

fn java_date(text: &str) -> Option<SqlValue> {
    let millis = java_date_millis(text)?;
    Some(
        SqlValue::java_sql_date(millis, java_sql_date_epoch_day(millis), text)
            .with_driver(Driver::Postgres),
    )
}

fn trim_money(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    match bytes[0] {
        b'(' => {
            let inner = text[1..].strip_suffix(')').unwrap_or(&text[1..]);
            Some(format!("-{}", inner.get(1..).unwrap_or_default()))
        }
        b'$' => Some(text[1..].to_string()),
        b'-' if bytes[1] == b'$' => Some(format!("-{}", &text[2..])),
        _ => None,
    }
}

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

    pub(crate) fn decode(&self, raw: Option<&[u8]>) -> SqlValue {
        let Some(raw) = raw else {
            return SqlValue::Null;
        };
        self.decode_text(String::from_utf8_lossy(raw))
    }

    fn decode_text(&self, text: Cow<'_, str>) -> SqlValue {
        let unparsed = |text: Cow<'_, str>| SqlValue::String(text.into_owned());
        match self {
            Decoder::Text => match trim_money(&text) {
                Some(trimmed) => rendered(SqlValue::String(trimmed), text.into_owned()),
                None => unparsed(text),
            },
            Decoder::Bool => match text.as_ref() {
                "t" => rendered(SqlValue::Boolean(true), text.into_owned()),
                "f" => rendered(SqlValue::Boolean(false), text.into_owned()),
                _ => unparsed(text),
            },
            Decoder::Int2 => text
                .parse()
                .map_or_else(|_| unparsed(text), SqlValue::SmallInt),
            Decoder::Int4 => text
                .parse()
                .map_or_else(|_| unparsed(text), SqlValue::Integer),
            Decoder::Int8 => text
                .parse()
                .map_or_else(|_| unparsed(text), SqlValue::BigInt),
            Decoder::Float4 => match parse_f32(&text) {
                Some(v) => rendered(SqlValue::Real(v), text.into_owned()),
                None => unparsed(text),
            },
            Decoder::Float8 => match parse_f64(&text) {
                Some(v) => rendered(SqlValue::Double(v), text.into_owned()),
                None => unparsed(text),
            },
            Decoder::Numeric => match BigDecimal::from_str(&text) {
                Ok(d) if big_decimal_to_string(&d) == text => SqlValue::Decimal(d),
                Ok(d) => rendered(SqlValue::Decimal(d), text.into_owned()),
                Err(_) => match parse_f64(&text) {
                    Some(v) if v.is_nan() || v.is_infinite() => {
                        rendered(SqlValue::Double(v), text.into_owned())
                    }
                    _ => unparsed(text),
                },
            },
            Decoder::Date => java_date(&text).unwrap_or_else(|| invalid_date_time(text)),
            Decoder::Timestamp | Decoder::TimestampTz => {
                java_timestamp(&text, *self == Decoder::TimestampTz)
                    .unwrap_or_else(|| invalid_date_time(text))
            }
            Decoder::Bytea => decode_bytea(&text).map_or_else(|| unparsed(text), SqlValue::Bytes),
            Decoder::Array { element, delimiter } => match parse_array(&text, *delimiter) {
                Some(items) => rendered(
                    SqlValue::Array(element.decode_items(items)),
                    text.into_owned(),
                ),
                None => unparsed(text),
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
        match self {
            Decoder::Date => java_date(&text).unwrap_or(SqlValue::String(text)),
            Decoder::Numeric if BigDecimal::from_str(&text).is_err() => {
                SqlValue::invalid_element("BigDecimal", text).with_driver(Driver::Postgres)
            }
            Decoder::Timestamp | Decoder::TimestampTz => {
                java_timestamp(&text, *self == Decoder::TimestampTz)
                    .unwrap_or(SqlValue::String(text))
            }
            _ => match self.decode_text(Cow::Owned(text)) {
                SqlValue::Rendered { value, .. } => *value,
                other => other,
            },
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
    let items = parse_level(bytes, &mut pos, delimiter, 1)?;
    (pos == bytes.len()).then_some(items)
}

fn parse_level(
    bytes: &[u8],
    pos: &mut usize,
    delimiter: u8,
    depth: usize,
) -> Option<Vec<ArrayItem>> {
    if bytes.get(*pos) != Some(&b'{') || depth > MAX_ARRAY_DEPTH {
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
            b'{' => items.push(ArrayItem::Nested(parse_level(
                bytes,
                pos,
                delimiter,
                depth + 1,
            )?)),
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
                if raw == "NULL" {
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

fn java_time_text(date: NaiveDate, time: &str) -> String {
    let (year, era) = if date.year() > 0 {
        (date.year(), "")
    } else {
        (1 - date.year(), " BC")
    };
    format!("{year:04}-{:02}-{:02}{time}{era}", date.month(), date.day())
}

fn timestamp_text(timestamp: &NaiveDateTime) -> Result<String, SqlError> {
    let out_of_range = || SqlError::new(TIMESTAMP_OUT_OF_RANGE);
    let rounded = if timestamp.nanosecond() % NANOS_PER_MICRO >= NANOS_PER_MICRO / 2 {
        timestamp
            .checked_add_signed(TimeDelta::microseconds(1))
            .ok_or_else(out_of_range)?
    } else {
        *timestamp
    };
    if Some(rounded.date()) < MIN_TIMESTAMP_DATE {
        return Err(out_of_range());
    }
    let micros = rounded.nanosecond() / NANOS_PER_MICRO;
    let fraction = if micros == 0 {
        String::new()
    } else {
        format!(".{micros:06}").trim_end_matches('0').to_string()
    };
    Ok(java_time_text(
        rounded.date(),
        &format!(" {}{fraction}+00", rounded.format("%H:%M:%S")),
    ))
}

pub(crate) fn check_parameters(parameters: &[SqlValue]) -> Result<(), SqlError> {
    for parameter in parameters {
        encode_parameter(parameter, false)?;
    }
    Ok(())
}

pub(crate) fn encode_parameter(
    value: &SqlValue,
    string_unspecified: bool,
) -> Result<(u32, Option<String>), SqlError> {
    Ok(match value {
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
        SqlValue::Date(d) => (oid::DATE, Some(java_time_text(*d, ""))),
        SqlValue::Time(t) => (0, Some(t.format("%H:%M:%S%.f").to_string())),
        SqlValue::Timestamp(ts) => (0, Some(timestamp_text(ts)?)),
        SqlValue::TimestampTz(ts) => (
            0,
            Some(format!("{}+00", ts.naive_utc().format(TIMESTAMP_FORMAT))),
        ),
        SqlValue::Array(_) => (0, Some(value.to_string())),
        SqlValue::Rendered { value, .. } => encode_parameter(value, string_unspecified)?,
    })
}

#[cfg(test)]
mod tests {
    use athena_federation_sdk::exceptions::JavaThrowable;

    fn encoded(value: &SqlValue, string_unspecified: bool) -> (u32, Option<String>) {
        encode_parameter(value, string_unspecified).unwrap()
    }

    use super::*;
    use athena_jdbc::sql::JdbcType;
    use chrono::{NaiveDate, NaiveDateTime};

    fn cache() -> TypeCache {
        TypeCache::default()
    }

    #[test]
    fn booleans_keep_the_server_text() {
        let value = Decoder::Bool.decode(Some(b"t"));
        assert_eq!(value.to_boolean().unwrap(), Some(true));
        assert_eq!(value.to_java_string().as_deref(), Some("t"));
    }

    #[test]
    fn timestamps_with_time_zone_parse_short_offsets() {
        let value = Decoder::TimestampTz.decode(Some(b"2000-01-02 12:59:00+00"));
        assert_eq!(
            value.to_timestamp().unwrap().unwrap().to_string(),
            "2000-01-02 12:59:00"
        );
        assert_eq!(
            value.to_java_string().as_deref(),
            Some("2000-01-02 12:59:00+00")
        );
        let shifted = Decoder::TimestampTz.decode(Some(b"2000-01-01 05:30:00+05:30"));
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
        let value = decoder.decode(Some(b"{7,8,9}"));
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
        let value = Decoder::Numeric.decode(Some(b"0.0000001"));
        assert_eq!(value.to_java_string().as_deref(), Some("0.0000001"));
        assert_eq!(
            value.to_big_decimal().unwrap().unwrap(),
            BigDecimal::from_str("0.0000001").unwrap()
        );
        let plain = Decoder::Numeric.decode(Some(b"86.4199"));
        assert!(matches!(plain, SqlValue::Decimal(_)));
        assert_eq!(plain.to_long().unwrap(), Some(86));
    }

    #[test]
    fn parameters_use_pgjdbc_type_oids() {
        assert_eq!(encoded(&SqlValue::Real(0.1), false).0, oid::FLOAT4);
        assert_eq!(encoded(&SqlValue::Double(0.1), false).0, oid::FLOAT8);
        assert_eq!(
            encoded(&SqlValue::String("x".into()), false),
            (oid::VARCHAR, Some("x".to_string()))
        );
        assert_eq!(encoded(&SqlValue::String("x".into()), true).0, 0);
        assert_eq!(
            encoded(
                &SqlValue::Date(NaiveDate::from_ymd_opt(2020, 1, 2).unwrap()),
                false
            ),
            (oid::DATE, Some("2020-01-02".to_string()))
        );
        assert_eq!(
            encoded(&SqlValue::Boolean(true), false),
            (oid::BOOL, Some("TRUE".to_string()))
        );
        assert_eq!(JdbcType::Array, cache().sql_type(1007));
    }

    #[test]
    fn date_and_timestamp_parameters_bind_like_the_java_query_builder() {
        let date = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(
            encoded(&SqlValue::Date(date(-43, 3, 15)), false),
            (oid::DATE, Some("0044-03-15 BC".to_string()))
        );
        assert_eq!(
            encoded(&SqlValue::Date(date(0, 2, 29)), false),
            (oid::DATE, Some("0001-02-29 BC".to_string()))
        );
        assert_eq!(
            encoded(
                &SqlValue::Timestamp(date(1000, 1, 1).and_hms_opt(12, 0, 0).unwrap()),
                false
            ),
            (0, Some("1000-01-01 12:00:00+00".to_string()))
        );
        assert_eq!(
            encoded(
                &SqlValue::Timestamp(date(-43, 3, 15).and_hms_milli_opt(12, 0, 0, 500).unwrap()),
                false
            ),
            (0, Some("0044-03-15 12:00:00.5+00 BC".to_string()))
        );
    }

    #[test]
    fn bind_timestamp_sends_utc_text_with_unspecified_type() {
        let cases = [
            ("1000-01-01T12:00:00", "1000-01-01 12:00:00+00"),
            (
                "2020-06-01T06:30:00.123456789",
                "2020-06-01 06:30:00.123457+00",
            ),
            ("-0043-03-15T12:00:00", "0044-03-15 12:00:00+00 BC"),
            ("0000-02-29T06:30:00.500", "0001-02-29 06:30:00.5+00 BC"),
            (
                "2020-06-01T06:30:00.000002500",
                "2020-06-01 06:30:00.000003+00",
            ),
            (
                "2020-06-01T06:30:00.000002499",
                "2020-06-01 06:30:00.000002+00",
            ),
            ("2020-06-01T06:30:00.000000400", "2020-06-01 06:30:00+00"),
            ("2020-06-01T06:30:00.999999500", "2020-06-01 06:30:01+00"),
            ("2020-06-01T23:59:59.999999700", "2020-06-02 00:00:00+00"),
            (
                "-4713-11-23T23:59:59.999999500",
                "4714-11-24 00:00:00+00 BC",
            ),
            ("+10000-01-01T00:00:00", "10000-01-01 00:00:00+00"),
            ("-4712-01-01T00:00:00", "4713-01-01 00:00:00+00 BC"),
            ("-4713-12-31T23:59:59", "4714-12-31 23:59:59+00 BC"),
            ("-4713-11-24T00:00:00", "4714-11-24 00:00:00+00 BC"),
        ];
        for (timestamp, text) in cases {
            let value = SqlValue::Timestamp(timestamp.parse().unwrap());
            assert_eq!((0, Some(text.to_string())), encoded(&value, false));
        }
    }

    #[test]
    fn bind_timestamp_rejects_values_outside_the_postgre_sql_range() {
        let cases = [
            "-4713-11-23T23:59:59.999999",
            "-4714-11-24T00:00:00",
            "-5000-01-01T00:00:00",
            "-262143-01-01T00:00:00",
        ];
        let mut values: Vec<NaiveDateTime> =
            cases.iter().map(|text| text.parse().unwrap()).collect();
        values.extend([NaiveDateTime::MIN, NaiveDateTime::MAX]);
        for value in values {
            let error = encode_parameter(&SqlValue::Timestamp(value), false).unwrap_err();
            assert_eq!(
                "Timestamp is outside the PostgreSQL range 4714-11-24 00:00:00 BC to 294276-12-31 23:59:59.999999",
                error.message(),
                "{value}"
            );
            assert!(check_parameters(&[SqlValue::Integer(1), SqlValue::Timestamp(value)]).is_err());
        }
        assert!(
            check_parameters(&[SqlValue::Timestamp("-4713-11-24T00:00:00".parse().unwrap())])
                .is_ok()
        );
    }

    fn epoch_day(value: &SqlValue) -> i64 {
        value.to_epoch_day().unwrap().unwrap()
    }

    #[test]
    fn timestamptz_dateday_uses_the_default_zone() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "driver::codec::tests::timestamptz_dateday_in_los_angeles",
                "--ignored",
            ])
            .env("TZ", "America/Los_Angeles")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{stdout}");
        assert!(stdout.contains("test result: ok. 1 passed"), "{stdout}");
    }

    #[test]
    #[ignore = "timestamptz_dateday_uses_the_default_zone runs it with TZ=America/Los_Angeles"]
    fn timestamptz_dateday_in_los_angeles() {
        assert_eq!(Ok("America/Los_Angeles"), std::env::var("TZ").as_deref());
        let day = |y, m, d| {
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .signed_duration_since(NaiveDate::default())
                .num_days()
        };
        let cases = [
            ("2020-06-01 03:00:00+00", day(2020, 5, 31), day(2020, 6, 1)),
            ("2020-06-01 08:00:00+00", day(2020, 6, 1), day(2020, 6, 1)),
            ("2100-07-01 07:30:00+00", day(2100, 7, 1), day(2100, 7, 1)),
            ("2100-01-01 07:30:00+00", day(2099, 12, 31), day(2100, 1, 1)),
        ];
        for (text, zoned, unzoned) in cases {
            assert_eq!(
                zoned,
                epoch_day(&Decoder::TimestampTz.decode(Some(text.as_bytes()))),
                "{text}"
            );
            let local = text.trim_end_matches("+00");
            assert_eq!(
                unzoned,
                epoch_day(&Decoder::Timestamp.decode(Some(local.as_bytes()))),
                "{text}"
            );
        }
    }

    fn millis(value: &SqlValue) -> i64 {
        value.to_timestamp_millis().unwrap().unwrap()
    }

    fn millis_of(value: &SqlValue) -> i64 {
        value.to_epoch_millis().unwrap().unwrap()
    }

    fn array_value(oid: u32, text: &str) -> athena_federation_sdk::data::Value {
        let value = Decoder::for_type(&cache(), oid).decode(Some(text.as_bytes()));
        athena_federation_sdk::data::Value::List(
            value
                .to_array()
                .unwrap()
                .unwrap()
                .iter()
                .map(SqlValue::to_value)
                .collect(),
        )
    }

    #[test]
    fn dates_decode_to_the_java_sql_date_pgjdbc_produces() {
        let day = |text: &str| epoch_day(&Decoder::Date.decode(Some(text.as_bytes())));
        assert_eq!(day("0001-01-01 BC"), -719_528);
        assert_eq!(day("infinity"), 106_751_991_166);
        assert_eq!(day("-infinity"), -106_749_799_148);
        assert_eq!(day("10000-01-01"), 2_932_897);
        assert_eq!(day("1582-10-04"), -141_438);
        assert_eq!(day("9999-12-31"), 2_932_896);
        let value = Decoder::Date.decode(Some(b"infinity"));
        assert_eq!(value.to_java_string().as_deref(), Some("infinity"));
    }

    #[test]
    fn timestamps_decode_to_the_proleptic_day() {
        let day = |decoder: Decoder, text: &str| epoch_day(&decoder.decode(Some(text.as_bytes())));
        let expected = |y, m, d| {
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .signed_duration_since(NaiveDate::default())
                .num_days()
        };
        assert_eq!(
            day(Decoder::Timestamp, "1000-01-01 12:00:00"),
            expected(1000, 1, 1)
        );
        assert_eq!(
            day(Decoder::Timestamp, "0044-03-15 12:00:00 BC"),
            expected(-43, 3, 15)
        );
        assert_eq!(
            day(Decoder::Timestamp, "1582-10-04 23:59:59.999"),
            expected(1582, 10, 4)
        );
        assert_eq!(
            day(Decoder::TimestampTz, "2020-06-01 03:00:00+05:30"),
            expected(2020, 5, 31)
        );
        assert_eq!(
            day(Decoder::TimestampTz, "1582-10-15 03:00:00+05:30"),
            expected(1582, 10, 14)
        );
        assert_eq!(
            day(Decoder::Timestamp, "1582-10-10 12:00:00"),
            expected(1582, 10, 10)
        );
        assert_eq!(
            day(Decoder::TimestampTz, "1582-10-10 12:00:00+00"),
            expected(1582, 10, 10)
        );
        assert_eq!(day(Decoder::Timestamp, "infinity"), 106_751_991_166);
        assert_eq!(day(Decoder::TimestampTz, "-infinity"), -106_751_991_168);
    }

    #[test]
    fn bc_timestamptz_reads_the_same_instant_in_every_session_zone() {
        for text in [
            "0044-03-15 12:00:00+00 BC",
            "0044-03-15 17:53:28+05:53:28 BC",
            "0044-03-15 04:07:02-07:52:58 BC",
        ] {
            let value = Decoder::TimestampTz.decode(Some(text.as_bytes()));
            assert_eq!(millis_of(&value), -63_517_780_800_000, "{text}");
        }
    }

    #[test]
    fn bc_dates_decode_on_the_proleptic_calendar() {
        let day = |text: &str| epoch_day(&Decoder::Date.decode(Some(text.as_bytes())));
        for (text, (year, month, date)) in [
            ("0001-01-01 BC", (0, 1, 1)),
            ("0001-02-29 BC", (0, 2, 29)),
            ("0002-03-01 BC", (-1, 3, 1)),
            ("0044-03-15 BC", (-43, 3, 15)),
            ("4713-11-24 BC", (-4712, 11, 24)),
        ] {
            let expected = NaiveDate::from_ymd_opt(year, month, date)
                .unwrap()
                .signed_duration_since(NaiveDate::default())
                .num_days();
            assert_eq!(day(text), expected, "{text}");
        }
    }

    #[test]
    fn get_timestamp_millis_reads_pre1582_timestamps_as_proleptic_millis() {
        let epoch = |decoder: Decoder, text: &str| {
            decoder
                .decode(Some(text.as_bytes()))
                .to_epoch_millis()
                .unwrap()
                .unwrap()
        };
        let cases = [
            ("1000-01-01 12:00:00", -30_610_180_800_000),
            ("0044-03-15 12:00:00 BC", -63_517_780_800_000),
            ("0001-02-29 06:30:00.5 BC", -62_162_098_199_500),
            ("1582-10-04 23:59:59.999", -12_220_156_800_001),
            ("1582-10-15 00:00:00", -12_219_292_800_000),
            ("2020-06-01 12:00:00.123456+05:30", 1_590_993_000_123),
            ("infinity", 9_223_372_036_825_200_000),
            ("-infinity", -9_223_372_036_832_400_000),
        ];
        for decoder in [Decoder::Timestamp, Decoder::TimestampTz] {
            for (text, millis) in cases {
                assert_eq!(millis, epoch(decoder.clone(), text), "{decoder:?} {text}");
            }
        }
        assert_eq!(
            -62_167_219_200_000,
            epoch(Decoder::TimestampTz, "0001-01-01 00:00:00+00 BC")
        );
        assert_eq!(
            -2_208_971_038_000,
            epoch(Decoder::TimestampTz, "1900-01-01 00:00:00-04:56:02")
        );
        assert_eq!(
            253_402_300_800_000,
            epoch(Decoder::Timestamp, "10000-01-01 00:00:00")
        );
        assert_eq!(
            Some(-30_609_748_800_000),
            Decoder::Timestamp
                .decode(Some(b"1000-01-01 12:00:00"))
                .to_timestamp_millis()
                .unwrap()
        );
    }

    #[test]
    fn timestamps_decode_to_java_sql_timestamp_millis() {
        let ts = |text: &str| millis(&Decoder::Timestamp.decode(Some(text.as_bytes())));
        assert_eq!(ts("1000-01-01 12:00:00"), -30_609_748_800_000);
        assert_eq!(ts("0044-03-15 12:00:00 BC"), -63_517_953_600_000);
        assert_eq!(ts("1582-10-04 23:59:59.999"), -12_219_292_800_001);
        assert_eq!(ts("10000-01-01 00:00:00"), 253_402_300_800_000);
        assert_eq!(ts("infinity"), 9_223_372_036_825_200_000);
        let tz = |text: &str| millis(&Decoder::TimestampTz.decode(Some(text.as_bytes())));
        assert_eq!(tz("-infinity"), -9_223_372_036_832_400_000);
        assert_eq!(tz("0001-01-01 00:00:00+00 BC"), -719_530 * 86_400_000);
        assert_eq!(tz("1900-01-01 00:00:00-04:56:02"), -2_208_971_038_000);
        let infinite = Decoder::Timestamp.decode(Some(b"infinity"));
        assert_eq!(
            infinite.to_timestamp().unwrap_err().message(),
            "Bad value for type timestamp : infinity"
        );
        assert_eq!(
            infinite.to_timestamp().unwrap_err().java_class_name(),
            "org.postgresql.util.PSQLException"
        );
    }

    const MALFORMED_DATE_TIME_TEXT: [&str; 12] = [
        "2020-01-01 00:00:00+9999999999999999",
        "9223372036854775807-01-01 00:00:00",
        "2020-01-01 2562047788015216:00:00",
        "2147483647-01-01 00:00:00",
        "292278995-01-01 00:00:00",
        "2020-01-01 00:00:00+99",
        "2020-01-01 00:00:00+24:00",
        "2020-01-01 00:00:00+2147483647",
        "2020-01-01 00:00:00.9999999999",
        "2020-01-01 00:00:00x",
        "abc",
        "9223372036854775807-01-01",
    ];

    const ACCEPTED_DATE_TIME_TEXT: [(&str, i64); 8] = [
        ("2020-01-01 2147483647:00:00", 7_732_518_966_000_000),
        ("292278994-08-17 07:12:55.807", i64::MAX),
        ("2020-13-01 00:00:00", 1_609_459_200_000),
        ("2020-01-01 00:00:00 BC", -125_882_121_600_000),
        ("2020-01-01", 1_577_836_800_000),
        ("2020-1-1 1:2:3", 1_577_840_523_000),
        ("2020-01-01 00:00:00+05:30:15", 1_577_816_985_000),
        ("infinity", 9_223_372_036_825_200_000),
    ];

    #[test]
    fn malformed_date_time_text_fails_like_the_java_connector() {
        for text in MALFORMED_DATE_TIME_TEXT {
            for decoder in [Decoder::Timestamp, Decoder::TimestampTz, Decoder::Date] {
                let value = decoder.decode(Some(text.as_bytes()));
                let expected = format!("Bad value for type timestamp/date/time: {text}");
                for error in [
                    value.to_timestamp().unwrap_err(),
                    value.to_timestamp_millis().unwrap_err(),
                    value.to_date().unwrap_err(),
                ] {
                    assert_eq!(expected, error.message(), "{decoder:?} {text}");
                    assert!(
                        matches!(error, SqlError::Postgres { .. }),
                        "{decoder:?} {text}"
                    );
                }
                assert_eq!(Some(text.to_string()), value.to_java_string());
            }
        }
    }

    #[test]
    fn check_date_time_text_rejects_malformed_date_and_timestamp_text() {
        let malformed = "2147483647-01-01 00:00:00";
        for decoder in [Decoder::Date, Decoder::Timestamp, Decoder::TimestampTz] {
            let error = decoder
                .decode(Some(malformed.as_bytes()))
                .to_timestamp_millis()
                .unwrap_err();
            assert_eq!(
                format!("Bad value for type timestamp/date/time: {malformed}"),
                error.message()
            );
            assert!(matches!(error, SqlError::Postgres { .. }));
            assert!(decoder
                .decode(Some(b"2020-01-01 00:00:00"))
                .to_timestamp_millis()
                .is_ok());
            assert_eq!(None, decoder.decode(None).to_timestamp_millis().unwrap());
        }
        assert_eq!(
            Some(malformed.to_string()),
            Decoder::Text
                .decode(Some(malformed.as_bytes()))
                .to_java_string()
        );
    }

    #[test]
    fn lenient_date_time_text_keeps_the_pgjdbc_value() {
        for (text, millis) in ACCEPTED_DATE_TIME_TEXT {
            let value = Decoder::Timestamp.decode(Some(text.as_bytes()));
            assert_eq!(Some(millis), value.to_timestamp_millis().unwrap(), "{text}");
        }
    }

    #[test]
    fn date_arrays_write_java_date_millis_like_the_java_record_handler() {
        use athena_federation_sdk::data::Value;
        assert_eq!(
            array_value(1182, "{infinity}"),
            Value::List(vec![Value::Date(9_223_372_036_825_200_000)])
        );
        assert_eq!(
            array_value(1182, "{0001-01-01 BC,1970-01-01,NULL}"),
            Value::List(vec![
                Value::Date(-719_530 * 86_400_000),
                Value::Date(0),
                Value::Null
            ])
        );
    }

    #[test]
    fn numeric_nan_fails_like_pgjdbc() {
        let nan = Decoder::Numeric.decode(Some(b"NaN"));
        let error = nan.to_big_decimal().unwrap_err();
        assert_eq!(error.message(), "Bad value for type BigDecimal : NaN");
        assert_eq!(error.java_class_name(), "org.postgresql.util.PSQLException");
        assert!(nan.to_double().unwrap().unwrap().is_nan());
        let array = Decoder::for_type(&cache(), 1231).decode(Some(b"{1.5,NaN}"));
        let error = array.to_array().unwrap_err();
        assert_eq!(error.message(), "Bad value for type BigDecimal : NaN");
        assert_eq!(error.java_class_name(), "org.postgresql.util.PSQLException");
    }

    #[test]
    fn money_text_is_trimmed_for_numeric_getters() {
        let money = Decoder::Text.decode(Some(b"$1,001.50"));
        assert_eq!(money.to_java_string().as_deref(), Some("$1,001.50"));
        assert!(money.to_double().is_err());
        let money = Decoder::Text.decode(Some(b"-$1.50"));
        assert_eq!(money.to_double().unwrap(), Some(-1.5));
        assert_eq!(
            money.to_big_decimal().unwrap().unwrap().to_string(),
            "-1.50"
        );
        let money = Decoder::Text.decode(Some(b"($2.25)"));
        assert_eq!(money.to_double().unwrap(), Some(-2.25));
        assert_eq!(
            Decoder::Text.decode(Some(b"$")).to_java_string().as_deref(),
            Some("$")
        );
    }

    #[test]
    fn array_nesting_is_capped_at_six_levels() {
        let six = format!("{}1{}", "{".repeat(6), "}".repeat(6));
        assert!(parse_array(&six, b',').is_some());
        let seven = format!("{}1{}", "{".repeat(7), "}".repeat(7));
        assert!(parse_array(&seven, b',').is_none());
        let deep = format!("{}1{}", "{".repeat(100_000), "}".repeat(100_000));
        assert!(parse_array(&deep, b',').is_none());
    }
}
