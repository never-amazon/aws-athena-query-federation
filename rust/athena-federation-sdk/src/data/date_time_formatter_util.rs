use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use arrow::datatypes::{DataType, TimeUnit};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Duration, Months, NaiveDate, NaiveDateTime, Utc};
use num_traits::ToPrimitive;

use super::java_time::{ZoneId, ZonedDateTime};
use super::time_zone_key::TimeZoneKey;
use super::types::ArrowType;
use super::value::Value;
use crate::exceptions::{FederationError, Result};

const SUPPORTED_DATETIME_FORMAT: [&str; 5] = [
    "yyyy-MM-dd",
    "yyyy-MM-dd'T'HH:mm:ss",
    "yyyy-MM-dd'T'HH:mm:ssZZ",
    "yyyyMMdd'T'HHmmss",
    "yyyyMMdd'T'HHmmssZZ",
];

const TIME_ZONE_MASK: i64 = 0xFFF;
const MILLIS_SHIFT: i64 = 12;

static PACK_TIMEZONE: AtomicBool = AtomicBool::new(true);

thread_local! {
    static PACK_TIMEZONE_OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeStampMilliTzHolder {
    pub value: i64,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeStampMicroTzHolder {
    pub value: i64,
    pub timezone: String,
}

pub struct DateTimeFormatterUtil;

impl DateTimeFormatterUtil {
    pub fn override_timezone_packing(enabled: bool) {
        PACK_TIMEZONE_OVERRIDE.with(|c| c.set(Some(enabled)));
    }

    pub fn clear_timezone_packing_override() {
        PACK_TIMEZONE_OVERRIDE.with(|c| c.set(None));
    }

    pub fn is_pack_timezone() -> bool {
        PACK_TIMEZONE_OVERRIDE
            .with(std::cell::Cell::get)
            .unwrap_or_else(|| PACK_TIMEZONE.load(Ordering::SeqCst))
    }

    pub fn disable_timezone_packing() {
        tracing::info!("Timezone packing disabled");
        PACK_TIMEZONE.store(false, Ordering::SeqCst);
    }

    pub fn enable_timezone_packing() {
        tracing::info!("Timezone packing enabled");
        PACK_TIMEZONE.store(true, Ordering::SeqCst);
    }

    pub fn string_to_local_date(
        value: &str,
        date_format: Option<&str>,
        default_time_zone: &ZoneId,
    ) -> Option<NaiveDate> {
        let fmt = match date_format {
            Some(f) if !f.is_empty() => f,
            _ => {
                tracing::info!(
                    "Unable to parse {} as Date type due to invalid dateformat",
                    value
                );
                return None;
            }
        };
        let instant = string_to_date(value, fmt)?;
        Some(ZonedDateTime::of_instant(instant, default_time_zone.clone()).to_local_date())
    }

    pub fn string_to_zoned_date_time(
        value: &str,
        date_format: Option<&str>,
        default_time_zone: Option<&ZoneId>,
    ) -> Value {
        match ZonedDateTime::parse(value) {
            Ok(zdt) => {
                tracing::info!("Parsed {} to ZonedDateTime {}", value, zdt);
                return Value::ZonedDateTime(zdt);
            }
            Err(_) => {
                tracing::warn!(
                    "Unable to parse {} as ZonedDateTime type due to invalid date format, Falling back to LocalDateTime using timezone {:?}",
                    value,
                    default_time_zone.map(|z| z.get_id().to_string())
                );
            }
        }
        match default_time_zone {
            Some(zone) => DateTimeFormatterUtil::string_to_date_time(value, date_format, zone),
            None => Value::Null,
        }
    }

    pub fn string_to_date_time(
        value: &str,
        date_format: Option<&str>,
        default_time_zone: &ZoneId,
    ) -> Value {
        let fmt = match date_format {
            Some(f) if !f.is_empty() => f,
            _ => {
                tracing::warn!(
                    "Unable to parse {} as DateTime type due to invalid date format",
                    value
                );
                return Value::Null;
            }
        };
        match string_to_date(value, fmt) {
            Some(instant) => Value::LocalDateTime(
                ZonedDateTime::of_instant(instant, default_time_zone.clone()).to_local_date_time(),
            ),
            None => Value::Null,
        }
    }

    pub fn big_decimal_to_local_date(
        value: Option<&BigDecimal>,
        default_time_zone: Option<&ZoneId>,
    ) -> Option<NaiveDate> {
        let instant = big_decimal_to_date(value?)?;
        Some(ZonedDateTime::of_instant(instant, default_time_zone?.clone()).to_local_date())
    }

    pub fn big_decimal_to_local_date_time(
        value: Option<&BigDecimal>,
        default_time_zone: Option<&ZoneId>,
    ) -> Option<NaiveDateTime> {
        let instant = big_decimal_to_date(value?)?;
        Some(ZonedDateTime::of_instant(instant, default_time_zone?.clone()).to_local_date_time())
    }

    pub fn infer_date_time_format(value: &str) -> Option<&'static str> {
        for fmt in SUPPORTED_DATETIME_FORMAT {
            if parse_date_with_pattern(value, fmt).is_some() {
                tracing::info!("Inferred format {} for value {}", fmt, value);
                return Some(fmt);
            }
        }
        tracing::warn!("Failed to infer format for {}", value);
        None
    }

    pub fn pack_date_time_with_zone(zdt: &ZonedDateTime) -> Result<i64> {
        DateTimeFormatterUtil::pack_millis_with_zone(zdt.to_epoch_milli(), zdt.get_zone().get_id())
    }

    pub fn pack_millis_with_zone(millis_utc: i64, zone_id: &str) -> Result<i64> {
        let key = TimeZoneKey::get_time_zone_key_for_id(zone_id)?;
        Ok(DateTimeFormatterUtil::pack(millis_utc, key.get_key()))
    }

    pub fn pack(millis_utc: i64, time_zone_key: i16) -> i64 {
        (millis_utc << MILLIS_SHIFT) | (i64::from(time_zone_key) & TIME_ZONE_MASK)
    }

    pub fn unpack_millis_utc(date_time_with_time_zone: i64) -> i64 {
        date_time_with_time_zone >> MILLIS_SHIFT
    }

    pub fn unpack_zone_key(date_time_with_time_zone: i64) -> Result<TimeZoneKey> {
        TimeZoneKey::get_time_zone_key((date_time_with_time_zone & TIME_ZONE_MASK) as i16)
    }

    pub fn construct_zoned_date_time(
        epoch_timestamp: i64,
        arrow_type: &ArrowType,
    ) -> Result<ZonedDateTime> {
        let (unit, tz) = match arrow_type {
            DataType::Timestamp(unit, tz) => (*unit, tz.clone()),
            other => {
                return Err(FederationError::ClassCast(format!(
                    "{other:?} cannot be cast to ArrowType.Timestamp"
                )))
            }
        };
        let tz = tz.ok_or_else(|| FederationError::NullPointer("timezone is null".into()))?;
        let mut zone = ZoneId::of(&tz)?;
        let mut epoch = epoch_timestamp;
        if DateTimeFormatterUtil::is_pack_timezone() {
            if unit != TimeUnit::Millisecond {
                return Err(FederationError::not_supported(
                    "Unpacking is only supported for milliseconds",
                ));
            }
            let key = DateTimeFormatterUtil::unpack_zone_key(epoch)?;
            epoch = DateTimeFormatterUtil::unpack_millis_utc(epoch);
            zone = ZoneId::of(key.get_id())?;
        }
        let instant = instant_plus(epoch, unit)?;
        Ok(ZonedDateTime::of_instant(instant, zone))
    }

    pub fn arrow_time_unit_to_nanos(unit: TimeUnit) -> i64 {
        match unit {
            TimeUnit::Second => 1_000_000_000,
            TimeUnit::Millisecond => 1_000_000,
            TimeUnit::Microsecond => 1_000,
            TimeUnit::Nanosecond => 1,
        }
    }

    pub fn zoned_date_time_from_object(value: &Value) -> Result<ZonedDateTime> {
        match value {
            Value::ZonedDateTime(z) => Ok(z.clone()),
            Value::LocalDateTime(ldt) => Ok(ZonedDateTime::of_local(*ldt, ZoneId::utc())),
            Value::Date(ms) => ZonedDateTime::of_epoch_milli(*ms, ZoneId::utc()),
            other => Err(FederationError::not_supported(format!(
                "Type: class {} not supported",
                other.java_class_name()
            ))),
        }
    }

    pub fn timestamp_milli_tz_holder_from_object(
        value: &Value,
        target_time_zone_id: Option<&str>,
    ) -> Result<TimeStampMilliTzHolder> {
        let zdt = DateTimeFormatterUtil::zoned_date_time_from_object(value)?;
        let mut holder = TimeStampMilliTzHolder {
            timezone: zdt.get_zone().get_id().to_string(),
            value: zdt.to_epoch_milli(),
        };
        if DateTimeFormatterUtil::is_pack_timezone() {
            holder.value =
                DateTimeFormatterUtil::pack_millis_with_zone(holder.value, &holder.timezone)?;
        }
        if let Some(tz) = target_time_zone_id {
            holder.timezone = tz.to_string();
        }
        Ok(holder)
    }

    pub fn timestamp_micro_tz_holder_from_object(
        value: &Value,
        target_time_zone_id: Option<&str>,
    ) -> Result<TimeStampMicroTzHolder> {
        if DateTimeFormatterUtil::is_pack_timezone() {
            return Err(FederationError::not_supported(
                "Packing for TimeStampMicroTZ is not currently supported",
            ));
        }
        let zdt = DateTimeFormatterUtil::zoned_date_time_from_object(value)?;
        let zone = target_time_zone_id.map_or_else(
            || zdt.get_zone().get_id().to_string(),
            std::string::ToString::to_string,
        );
        let micros = zdt.get_epoch_second() * 1_000_000 + i64::from(zdt.get_nano() / 1000);
        Ok(TimeStampMicroTzHolder {
            timezone: zone,
            value: micros,
        })
    }
}

fn instant_plus(amount: i64, unit: TimeUnit) -> Result<DateTime<Utc>> {
    let overflow =
        || FederationError::Arithmetic(format!("long overflow adding {amount} {unit:?}"));
    let epoch = DateTime::<Utc>::UNIX_EPOCH;
    let delta = match unit {
        TimeUnit::Second => Duration::try_seconds(amount).ok_or_else(overflow)?,
        TimeUnit::Millisecond => Duration::try_milliseconds(amount).ok_or_else(overflow)?,
        TimeUnit::Microsecond => Duration::microseconds(amount),
        TimeUnit::Nanosecond => Duration::nanoseconds(amount),
    };
    epoch.checked_add_signed(delta).ok_or_else(overflow)
}

fn big_decimal_to_date(value: &BigDecimal) -> Option<DateTime<Utc>> {
    let (int_part, _) = value.with_scale(0).into_bigint_and_exponent();
    let millis = int_part.to_i64()?;
    let result = DateTime::<Utc>::from_timestamp_millis(millis);
    if result.is_none() {
        tracing::warn!("Invalid long value to transform to Date object");
    }
    result
}

fn string_to_date(value: &str, date_format: &str) -> Option<DateTime<Utc>> {
    let result = parse_date_with_pattern(value, date_format);
    if result.is_none() {
        tracing::warn!(
            "Encountered value {} that is not consistent with inferred date/datetime format {} - will skip in result",
            value,
            date_format
        );
    }
    result
}

#[derive(Debug, Clone, PartialEq)]
enum PatternToken {
    Field(char, usize),
    Literal(String),
}

fn tokenize_pattern(pattern: &str) -> Option<Vec<PatternToken>> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            let mut lit = String::new();
            i += 1;
            loop {
                if i >= chars.len() {
                    return None;
                }
                if chars[i] == '\'' {
                    if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        lit.push('\'');
                        i += 2;
                        continue;
                    }
                    i += 1;
                    break;
                }
                lit.push(chars[i]);
                i += 1;
            }
            if lit.is_empty() {
                lit.push('\'');
            }
            tokens.push(PatternToken::Literal(lit));
        } else if c.is_ascii_alphabetic() {
            let mut n = 0;
            while i < chars.len() && chars[i] == c {
                n += 1;
                i += 1;
            }
            tokens.push(PatternToken::Field(c, n));
        } else {
            tokens.push(PatternToken::Literal(c.to_string()));
            i += 1;
        }
    }
    Some(tokens)
}

fn is_numeric_field(token: &PatternToken) -> bool {
    match token {
        PatternToken::Field('M' | 'L', n) => *n < 3,
        PatternToken::Field(c, _) => matches!(
            c,
            'y' | 'Y' | 'u' | 'd' | 'D' | 'H' | 'k' | 'K' | 'h' | 'm' | 's' | 'S' | 'w' | 'W' | 'F'
        ),
        PatternToken::Literal(_) => false,
    }
}

const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
const DAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

fn parse_offset(chars: &[char], pos: &mut usize) -> Option<i64> {
    if *pos >= chars.len() {
        return None;
    }
    if chars[*pos] == 'Z' {
        *pos += 1;
        return Some(0);
    }
    let sign = match chars[*pos] {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let mut p = *pos + 1;
    let two = |p: usize| -> Option<i64> {
        if p + 2 <= chars.len() && chars[p].is_ascii_digit() && chars[p + 1].is_ascii_digit() {
            let digit = |c: char| i64::from(c.to_digit(10).unwrap_or(0));
            Some(digit(chars[p]) * 10 + digit(chars[p + 1]))
        } else {
            None
        }
    };
    let hours = two(p)?;
    p += 2;
    let mut minutes = 0;
    if p < chars.len() && chars[p] == ':' {
        if let Some(m) = two(p + 1) {
            minutes = m;
            p += 3;
        }
    } else if let Some(m) = two(p) {
        minutes = m;
        p += 2;
    }
    *pos = p;
    Some(sign * (hours * 60 + minutes) * 60)
}

struct DateFields {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    millis: i64,
    pm: Option<bool>,
    hour12: bool,
    offset_seconds: Option<i64>,
}

impl Default for DateFields {
    fn default() -> Self {
        DateFields {
            year: 1970,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            millis: 0,
            pm: None,
            hour12: false,
            offset_seconds: None,
        }
    }
}

impl DateFields {
    fn set_numeric(&mut self, c: char, width: usize, text: &str, n: i64) {
        match c {
            'y' | 'Y' | 'u' => {
                self.year = if width == 2 && text.len() == 2 {
                    let now_year = i64::from(chrono::Datelike::year(&Utc::now()));
                    let century = (now_year - 80) / 100 * 100;
                    let mut y = century + n;
                    if y < now_year - 80 {
                        y += 100;
                    }
                    y
                } else {
                    n
                }
            }
            'M' | 'L' => self.month = n,
            'd' => self.day = n,
            'D' => {
                self.month = 1;
                self.day = n;
            }
            'H' => self.hour = n,
            'k' => self.hour = if n == 24 { 0 } else { n },
            'K' => {
                self.hour = n;
                self.hour12 = true;
            }
            'h' => {
                self.hour = if n == 12 { 0 } else { n };
                self.hour12 = true;
            }
            'm' => self.minute = n,
            's' => self.second = n,
            'S' => self.millis = n,
            _ => {}
        }
    }

    fn set_text(&mut self, c: char, width: usize, chars: &[char], pos: &mut usize) -> Option<()> {
        let rest_lower = || chars[*pos..].iter().collect::<String>().to_lowercase();
        match c {
            'M' | 'L' => {
                let (i, len) = match_name(&rest_lower(), &MONTHS)?;
                self.month = i64::try_from(i).ok()? + 1;
                *pos += len;
            }
            'E' => {
                let (_, len) = match_name(&rest_lower(), &DAYS)?;
                *pos += len;
            }
            'a' => {
                let rest = rest_lower();
                if rest.starts_with("am") {
                    self.pm = Some(false);
                } else if rest.starts_with("pm") {
                    self.pm = Some(true);
                } else {
                    return None;
                }
                *pos += 2;
            }
            'Z' | 'X' | 'x' | 'z' => {
                if c == 'z' || (c == 'Z' && width != 2) {
                    let rest: String = chars[*pos..].iter().collect();
                    if rest.starts_with("GMT") || rest.starts_with("UTC") {
                        *pos += 3;
                        if *pos >= chars.len() || (chars[*pos] != '+' && chars[*pos] != '-') {
                            self.offset_seconds = Some(0);
                        }
                    }
                    if self.offset_seconds.is_some() {
                        return Some(());
                    }
                }
                self.offset_seconds = Some(parse_offset(chars, pos)?);
            }
            'G' => {
                let rest = chars[*pos..].iter().collect::<String>().to_uppercase();
                if rest.starts_with("AD") || rest.starts_with("BC") {
                    *pos += 2;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
        Some(())
    }

    fn to_utc(&self) -> Option<DateTime<Utc>> {
        let hour = if self.hour12 && self.pm == Some(true) {
            self.hour + 12
        } else {
            self.hour
        };
        let base = NaiveDate::from_ymd_opt(i32::try_from(self.year).ok()?, 1, 1)?;
        let date = if self.month >= 1 {
            base.checked_add_months(Months::new(u32::try_from(self.month - 1).ok()?))?
        } else {
            base.checked_sub_months(Months::new(u32::try_from(1 - self.month).ok()?))?
        };
        let dt = date.and_hms_opt(0, 0, 0)?
            + Duration::try_days(self.day - 1)?
            + Duration::try_hours(hour)?
            + Duration::try_minutes(self.minute)?
            + Duration::try_seconds(self.second)?
            + Duration::try_milliseconds(self.millis)?;
        let utc = dt - Duration::try_seconds(self.offset_seconds.unwrap_or(0))?;
        Some(utc.and_utc())
    }
}

fn match_name(rest: &str, names: &[&str]) -> Option<(usize, usize)> {
    names.iter().enumerate().find_map(|(i, name)| {
        if rest.starts_with(name) {
            Some((i, name.len()))
        } else if rest.starts_with(&name[..3]) {
            Some((i, 3))
        } else {
            None
        }
    })
}

fn read_number(chars: &[char], pos: &mut usize, width: usize, fixed: bool) -> Option<String> {
    let start = *pos;
    if *pos < chars.len() && (chars[*pos] == '-' || chars[*pos] == '+') && !fixed {
        *pos += 1;
    }
    if fixed {
        let end = *pos + width;
        if end > chars.len() || !chars[*pos..end].iter().all(char::is_ascii_digit) {
            return None;
        }
        *pos = end;
    } else {
        let digits_start = *pos;
        while *pos < chars.len() && chars[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos == digits_start {
            return None;
        }
    }
    Some(chars[start..*pos].iter().collect())
}

fn parse_date_with_pattern(value: &str, pattern: &str) -> Option<DateTime<Utc>> {
    let tokens = tokenize_pattern(pattern)?;
    let chars: Vec<char> = value.chars().collect();
    let mut pos = 0usize;
    let mut fields = DateFields::default();
    for (idx, token) in tokens.iter().enumerate() {
        match token {
            PatternToken::Literal(lit) => {
                for lc in lit.chars() {
                    if pos >= chars.len() || chars[pos] != lc {
                        return None;
                    }
                    pos += 1;
                }
            }
            PatternToken::Field(c, width) if is_numeric_field(token) => {
                let fixed = tokens.get(idx + 1).is_some_and(is_numeric_field);
                let text = read_number(&chars, &mut pos, *width, fixed)?;
                let n: i64 = text.parse().ok()?;
                fields.set_numeric(*c, *width, &text, n);
            }
            PatternToken::Field(c, width) => fields.set_text(*c, *width, &chars, &mut pos)?,
        }
    }
    if pos != chars.len() {
        return None;
    }
    fields.to_utc()
}
