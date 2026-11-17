use std::cmp::Ordering;
use std::fmt;

use bigdecimal::BigDecimal;
use chrono::{Datelike, NaiveDate};
use num_bigint::{BigInt, Sign};

use crate::java_cast::long_to_int;

const DAYS_FROM_CE_TO_EPOCH: i64 = 719_163;
const MILLIS_PER_DAY: i64 = 86_400_000;

fn strip_fraction(v: &str) -> String {
    let mut s = v.to_string();
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

fn with_fraction(base: &str, fraction: &str) -> String {
    let head = match base.find('.') {
        Some(i) => &base[..i],
        None => base,
    };
    let trimmed = fraction.trim_end_matches('0');
    if trimmed.is_empty() {
        head.to_string()
    } else {
        format!("{head}.{trimmed}")
    }
}

fn round_to(v: &str, base_len: usize, precision: usize) -> String {
    let target = base_len + 1 + precision;
    if v.len() <= target {
        return v.to_string();
    }
    let mut s = v[..target].to_string();
    while s.len() > base_len && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    s
}

fn fixed_precision(v: &str, base_len: usize, precision: usize) -> String {
    let p = if v.len() <= base_len {
        0
    } else {
        v.len() - base_len - 1
    };
    match precision.cmp(&p) {
        Ordering::Less => fixed_precision(&round_to(v, base_len, precision), base_len, precision),
        Ordering::Greater => {
            let mut s = v.to_string();
            if p == 0 {
                s.push('.');
            }
            s.push_str(&"0".repeat(precision - p));
            s
        }
        Ordering::Equal => v.to_string(),
    }
}

fn ymd_from_days(days: i64) -> (i32, u32, u32) {
    let d = i32::try_from(days + DAYS_FROM_CE_TO_EPOCH)
        .ok()
        .and_then(NaiveDate::from_num_days_from_ce_opt)
        .unwrap_or(NaiveDate::MIN);
    (d.year(), d.month(), d.day())
}

fn millis_of_second(millis: i64) -> u32 {
    u32::try_from(millis.rem_euclid(1000)).unwrap_or_default()
}

fn digit_count(digits: &str) -> i64 {
    i64::try_from(digits.len()).unwrap_or(i64::MAX)
}

fn days_from_ymd(y: i32, m: u32, d: u32) -> i64 {
    NaiveDate::from_ymd_opt(y, m, d).map_or(0, |nd| {
        i64::from(nd.num_days_from_ce()) - DAYS_FROM_CE_TO_EPOCH
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DateString(String);

impl DateString {
    pub fn new(v: impl Into<String>) -> DateString {
        DateString(v.into())
    }

    pub fn from_days_since_epoch(days: i32) -> DateString {
        let (y, m, d) = ymd_from_days(i64::from(days));
        DateString(format!("{y:04}-{m:02}-{d:02}"))
    }

    pub fn get_days_since_epoch(&self) -> i32 {
        let y = self
            .0
            .get(0..4)
            .and_then(|s| s.parse().ok())
            .unwrap_or(1970);
        let m = self.0.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
        let d = self.0.get(8..10).and_then(|s| s.parse().ok()).unwrap_or(1);
        long_to_int(days_from_ymd(y, m, d))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DateString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimeString(String);

impl TimeString {
    pub fn new(v: impl Into<String>) -> TimeString {
        TimeString(strip_fraction(&v.into()))
    }

    pub fn from_millis_of_day(millis: i32) -> TimeString {
        let ms = i64::from(millis);
        let secs = ms.div_euclid(1000);
        let h = secs / 3600;
        let m = (secs / 60) % 60;
        let s = secs % 60;
        TimeString(format!("{h:02}:{m:02}:{s:02}")).with_millis(millis_of_second(ms))
    }

    #[must_use]
    pub fn with_millis(&self, millis: u32) -> TimeString {
        TimeString(with_fraction(&self.0, &format!("{millis:03}")))
    }

    #[must_use]
    pub fn with_nanos(&self, nanos: u32) -> TimeString {
        TimeString(with_fraction(&self.0, &format!("{nanos:09}")))
    }

    #[must_use]
    pub fn round(&self, precision: usize) -> TimeString {
        TimeString(round_to(&self.0, 8, precision))
    }

    pub fn to_string_with_precision(&self, precision: usize) -> String {
        fixed_precision(&self.0, 8, precision)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TimeString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimestampString(String);

impl TimestampString {
    pub fn new(v: impl Into<String>) -> TimestampString {
        TimestampString(strip_fraction(&v.into()))
    }

    pub fn from_millis_since_epoch(millis: i64) -> TimestampString {
        let days = millis.div_euclid(MILLIS_PER_DAY);
        let rem = millis.rem_euclid(MILLIS_PER_DAY);
        let (y, mo, d) = ymd_from_days(days);
        let secs = rem / 1000;
        let h = secs / 3600;
        let mi = (secs / 60) % 60;
        let s = secs % 60;
        TimestampString(format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"))
            .with_millis(millis_of_second(millis))
    }

    #[must_use]
    pub fn with_millis(&self, millis: u32) -> TimestampString {
        TimestampString(with_fraction(&self.0, &format!("{millis:03}")))
    }

    #[must_use]
    pub fn with_nanos(&self, nanos: u32) -> TimestampString {
        TimestampString(with_fraction(&self.0, &format!("{nanos:09}")))
    }

    #[must_use]
    pub fn round(&self, precision: usize) -> TimestampString {
        TimestampString(round_to(&self.0, 19, precision))
    }

    pub fn to_string_with_precision(&self, precision: usize) -> String {
        fixed_precision(&self.0, 19, precision)
    }

    pub fn get_millis_since_epoch(&self) -> i64 {
        let v = &self.0;
        let num =
            |a: usize, b: usize| -> i64 { v.get(a..b).and_then(|s| s.parse().ok()).unwrap_or(0) };
        let days = days_from_ymd(
            i32::try_from(num(0, 4)).unwrap_or_default(),
            u32::try_from(num(5, 7)).unwrap_or_default(),
            u32::try_from(num(8, 10)).unwrap_or_default(),
        );
        let ms = match v.len() {
            19 => 0,
            21 => num(20, 21) * 100,
            22 => num(20, 22) * 10,
            _ => num(20, 23),
        };
        days * MILLIS_PER_DAY
            + num(11, 13) * 3_600_000
            + num(14, 16) * 60_000
            + num(17, 19) * 1000
            + ms
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TimestampString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BitString {
    bytes: Vec<u8>,
}

impl BitString {
    pub fn create_from_bytes(bytes: &[u8]) -> BitString {
        BitString {
            bytes: bytes.to_vec(),
        }
    }

    pub fn as_byte_array(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bit_count(&self) -> usize {
        self.bytes.len() * 8
    }

    pub fn to_hex_string(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789ABCDEF";
        let mut s = String::with_capacity(self.bytes.len() * 2);
        for b in &self.bytes {
            s.push(char::from(HEX[usize::from(b >> 4)]));
            s.push(char::from(HEX[usize::from(b & 0x0F)]));
        }
        s
    }

    pub fn to_bit_string(&self) -> String {
        let mut s = String::with_capacity(self.bytes.len() * 8);
        for b in &self.bytes {
            for bit in (0..8).rev() {
                s.push(if (b >> bit) & 1 == 1 { '1' } else { '0' });
            }
        }
        s
    }
}

impl fmt::Display for BitString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_bit_string())
    }
}

pub fn java_big_decimal_to_string(value: &BigDecimal) -> String {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    let negative = unscaled.sign() == Sign::Minus;
    let digits = unscaled.magnitude().to_string();
    let adjusted = digit_count(&digits) - 1 - scale;
    let sign = if negative { "-" } else { "" };
    if scale >= 0 && adjusted >= -6 {
        return format!("{sign}{}", plain_digits(&digits, scale));
    }
    let mut out = String::from(sign);
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    }
    if adjusted != 0 {
        out.push('E');
        if adjusted > 0 {
            out.push('+');
        }
        out.push_str(&adjusted.to_string());
    }
    out
}

pub fn java_big_decimal_to_plain_string(value: &BigDecimal) -> String {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    let negative = unscaled.sign() == Sign::Minus;
    let digits = unscaled.magnitude().to_string();
    let sign = if negative { "-" } else { "" };
    if scale <= 0 {
        if digits == "0" {
            return "0".to_string();
        }
        let zeros = usize::try_from(scale.unsigned_abs()).unwrap_or(usize::MAX);
        return format!("{sign}{digits}{}", "0".repeat(zeros));
    }
    format!("{sign}{}", plain_digits(&digits, scale))
}

fn plain_digits(digits: &str, scale: i64) -> String {
    if scale <= 0 {
        return digits.to_string();
    }
    let scale = usize::try_from(scale).unwrap_or(usize::MAX);
    if digits.len() > scale {
        format!(
            "{}.{}",
            &digits[..digits.len() - scale],
            &digits[digits.len() - scale..]
        )
    } else {
        format!("0.{}{}", "0".repeat(scale - digits.len()), digits)
    }
}

pub fn java_big_decimal_to_scientific(value: &BigDecimal) -> String {
    let (unscaled, scale) = value.as_bigint_and_exponent();
    let negative = unscaled.sign() == Sign::Minus;
    let mut digits = unscaled.magnitude().to_string();
    let len = digit_count(&digits);
    let mut e = len - scale - 1;
    if unscaled == BigInt::from(0) {
        e = 0;
    }
    let mut ret = String::new();
    if negative {
        ret.push('-');
    }
    digits.truncate(20.min(digits.len()));
    ret.push_str(&digits[..1]);
    if scale == 0 {
        let trimmed = digits.trim_end_matches('0');
        digits = if trimmed.is_empty() {
            digits[..1].to_string()
        } else {
            trimmed.to_string()
        };
    }
    if digits.len() > 1 {
        ret.push('.');
        ret.push_str(&digits[1..]);
    }
    ret.push('E');
    ret.push_str(&e.to_string());
    ret
}

pub fn java_double_to_string(d: f64) -> String {
    if d.is_nan() {
        return "NaN".to_string();
    }
    if d.is_infinite() {
        return if d > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if d == 0.0 {
        return if d.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let abs = d.abs();
    if (1e-3..1e7).contains(&abs) {
        let s = format!("{d}");
        if s.contains('.') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        let s = format!("{d:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}

pub fn java_float_to_string(f: f32) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0.0".to_string()
        } else {
            "0.0".to_string()
        };
    }
    let abs = f.abs();
    if (1e-3..1e7).contains(&abs) {
        let s = format!("{f}");
        if s.contains('.') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        let s = format!("{f:e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((s.as_str(), "0"));
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{mantissa}.0")
        };
        format!("{mantissa}E{exp}")
    }
}
