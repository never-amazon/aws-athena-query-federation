use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset,
    TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ZoneRules {
    Fixed(FixedOffset),
    Region(Tz),
}

#[derive(Debug, Clone)]
pub struct ZoneId {
    id: String,
    is_offset: bool,
    rules: ZoneRules,
}

impl PartialEq for ZoneId {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.is_offset == other.is_offset
    }
}

impl Eq for ZoneId {}

impl Hash for ZoneId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

fn parse_offset_id(text: &str) -> Option<FixedOffset> {
    if text == "Z" {
        return FixedOffset::east_opt(0);
    }
    let sign = match text.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let rest = &text[1..];
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let colon_form = rest.contains(':');
    let (h, m, s) = match digits.len() {
        1 | 2 if !colon_form => (digits.parse::<i32>().ok()?, 0, 0),
        4 => (
            digits[0..2].parse::<i32>().ok()?,
            digits[2..4].parse::<i32>().ok()?,
            0,
        ),
        6 => (
            digits[0..2].parse::<i32>().ok()?,
            digits[2..4].parse::<i32>().ok()?,
            digits[4..6].parse::<i32>().ok()?,
        ),
        _ => return None,
    };
    if h > 18 || m > 59 || s > 59 {
        return None;
    }
    let total = sign * (h * 3600 + m * 60 + s);
    if total.abs() > 18 * 3600 {
        return None;
    }
    FixedOffset::east_opt(total)
}

pub fn offset_id(offset: &FixedOffset) -> String {
    let total = offset.local_minus_utc();
    if total == 0 {
        return "Z".to_string();
    }
    let sign = if total < 0 { '-' } else { '+' };
    let abs = total.abs();
    let h = abs / 3600;
    let m = (abs / 60) % 60;
    let s = abs % 60;
    if s != 0 {
        format!("{sign}{h:02}:{m:02}:{s:02}")
    } else {
        format!("{sign}{h:02}:{m:02}")
    }
}

impl ZoneId {
    pub fn of(id: &str) -> Result<ZoneId> {
        if id.is_empty() {
            return Err(FederationError::illegal_argument(
                "Invalid ID for ZoneId, empty",
            ));
        }
        if id.len() <= 1 || id.starts_with('+') || id.starts_with('-') {
            let offset = parse_offset_id(id).ok_or_else(|| {
                FederationError::illegal_argument(format!(
                    "Invalid ID for ZoneOffset, invalid format: {id}"
                ))
            })?;
            return Ok(ZoneId::of_offset(offset));
        }
        for prefix in ["UTC", "GMT", "UT"] {
            if id == prefix {
                return Ok(ZoneId {
                    id: id.to_string(),
                    is_offset: false,
                    rules: ZoneRules::Fixed(FixedOffset::east_opt(0).unwrap_or_else(|| Utc.fix())),
                });
            }
            if let Some(rest) = id.strip_prefix(prefix) {
                if rest.starts_with('+') || rest.starts_with('-') {
                    let offset = parse_offset_id(rest).ok_or_else(|| {
                        FederationError::illegal_argument(format!(
                            "Invalid ID for offset-based ZoneId: {id}"
                        ))
                    })?;
                    let normalized = if offset.local_minus_utc() == 0 {
                        prefix.to_string()
                    } else {
                        format!("{}{}", prefix, offset_id(&offset))
                    };
                    return Ok(ZoneId {
                        id: normalized,
                        is_offset: false,
                        rules: ZoneRules::Fixed(offset),
                    });
                }
            }
        }
        let tz = Tz::from_str(id)
            .map_err(|_| FederationError::Runtime(format!("Unknown time-zone ID: {id}")))?;
        Ok(ZoneId {
            id: id.to_string(),
            is_offset: false,
            rules: ZoneRules::Region(tz),
        })
    }

    pub fn of_offset(offset: FixedOffset) -> ZoneId {
        ZoneId {
            id: offset_id(&offset),
            is_offset: true,
            rules: ZoneRules::Fixed(offset),
        }
    }

    pub fn utc() -> ZoneId {
        ZoneId {
            id: "UTC".to_string(),
            is_offset: false,
            rules: ZoneRules::Fixed(Utc.fix()),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn is_offset(&self) -> bool {
        self.is_offset
    }

    pub fn offset_at(&self, instant: &DateTime<Utc>) -> FixedOffset {
        match self.rules {
            ZoneRules::Fixed(off) => off,
            ZoneRules::Region(tz) => {
                let utc = instant.naive_utc();
                tz.offset_from_utc_datetime(&(utc + rule_year_shift(&utc)))
                    .fix()
            }
        }
    }

    pub fn instant_of_local(&self, local: &NaiveDateTime) -> DateTime<Utc> {
        match self.rules {
            ZoneRules::Fixed(off) => Utc
                .from_utc_datetime(&(*local - Duration::seconds(i64::from(off.local_minus_utc())))),
            ZoneRules::Region(tz) => {
                let shift = rule_year_shift(local);
                let shifted = *local + shift;
                let offset = match tz.from_local_datetime(&shifted) {
                    LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.offset().fix(),
                    LocalResult::None => tz
                        .offset_from_utc_datetime(&(shifted - Duration::days(1)))
                        .fix(),
                };
                Utc.from_utc_datetime(
                    &(*local - Duration::seconds(i64::from(offset.local_minus_utc()))),
                )
            }
        }
    }
}

const FIRST_YEAR_PAST_RULE_TABLE: i32 = 2100;
const RULE_TABLE_CYCLE_YEARS: i32 = 28;

fn rule_year_shift(date_time: &NaiveDateTime) -> Duration {
    let year = date_time.year();
    if year < FIRST_YEAR_PAST_RULE_TABLE {
        return Duration::zero();
    }
    let layout = |y: i32| {
        NaiveDate::from_ymd_opt(y, 1, 1).map(|first| (first.weekday(), first.leap_year(), first))
    };
    let Some((weekday, leap, first)) = layout(year) else {
        return Duration::zero();
    };
    (FIRST_YEAR_PAST_RULE_TABLE - RULE_TABLE_CYCLE_YEARS..FIRST_YEAR_PAST_RULE_TABLE)
        .rev()
        .filter_map(layout)
        .find(|(w, l, _)| *w == weekday && *l == leap)
        .map_or_else(Duration::zero, |(_, _, same)| same - first)
}

impl fmt::Display for ZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id)
    }
}

#[derive(Debug, Clone)]
pub struct ZonedDateTime {
    instant: DateTime<Utc>,
    zone: ZoneId,
}

impl ZonedDateTime {
    pub fn of_instant(instant: DateTime<Utc>, zone: ZoneId) -> ZonedDateTime {
        ZonedDateTime { instant, zone }
    }

    pub fn of_local(local: NaiveDateTime, zone: ZoneId) -> ZonedDateTime {
        let instant = zone.instant_of_local(&local);
        ZonedDateTime { instant, zone }
    }

    pub fn of_epoch_milli(millis: i64, zone: ZoneId) -> Result<ZonedDateTime> {
        let instant = DateTime::<Utc>::from_timestamp_millis(millis).ok_or_else(|| {
            FederationError::Arithmetic(format!(
                "Instant exceeds minimum or maximum instant: {millis}"
            ))
        })?;
        Ok(ZonedDateTime { instant, zone })
    }

    pub fn parse(text: &str) -> Result<ZonedDateTime> {
        let err = || FederationError::Runtime(format!("Text '{text}' could not be parsed"));
        let (main, bracket) = match text.find('[') {
            Some(idx) => {
                if !text.ends_with(']') {
                    return Err(err());
                }
                (&text[..idx], Some(&text[idx + 1..text.len() - 1]))
            }
            None => (text, None),
        };
        let t_idx = main.find('T').ok_or_else(err)?;
        let after_t = &main[t_idx + 1..];
        let off_idx = after_t.find(['Z', '+', '-']).ok_or_else(err)?;
        let local_text = &main[..t_idx + 1 + off_idx];
        let offset_text = &after_t[off_idx..];
        let local = parse_iso_local_date_time(local_text).ok_or_else(err)?;
        let offset = parse_offset_id(offset_text).ok_or_else(err)?;
        let instant = Utc
            .from_utc_datetime(&(local - Duration::seconds(i64::from(offset.local_minus_utc()))));
        let zone = match bracket {
            Some(id) => ZoneId::of(id).map_err(|_| err())?,
            None => ZoneId::of_offset(offset),
        };
        if bracket.is_some() && zone.offset_at(&instant) != offset {
            return Ok(ZonedDateTime::of_local(local, zone));
        }
        Ok(ZonedDateTime { instant, zone })
    }

    pub fn zone(&self) -> &ZoneId {
        &self.zone
    }

    pub fn offset(&self) -> FixedOffset {
        self.zone.offset_at(&self.instant)
    }

    pub fn to_instant(&self) -> DateTime<Utc> {
        self.instant
    }

    pub fn to_epoch_milli(&self) -> i64 {
        self.instant.timestamp_millis()
    }

    pub fn epoch_second(&self) -> i64 {
        self.instant.timestamp()
    }

    pub fn nano(&self) -> u32 {
        self.instant.timestamp_subsec_nanos()
    }

    pub fn to_local_date_time(&self) -> NaiveDateTime {
        self.instant.naive_utc() + Duration::seconds(i64::from(self.offset().local_minus_utc()))
    }

    pub fn to_local_date(&self) -> NaiveDate {
        self.to_local_date_time().date()
    }

    #[must_use]
    pub fn with_zone_same_instant(&self, zone: ZoneId) -> ZonedDateTime {
        ZonedDateTime {
            instant: self.instant,
            zone,
        }
    }

    pub fn compare_to(&self, other: &ZonedDateTime) -> Ordering {
        self.instant
            .timestamp()
            .cmp(&other.instant.timestamp())
            .then(self.nano().cmp(&other.nano()))
            .then_with(|| self.to_local_date_time().cmp(&other.to_local_date_time()))
            .then_with(|| self.zone.id.cmp(&other.zone.id))
    }
}

impl PartialEq for ZonedDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.instant == other.instant && self.zone == other.zone
    }
}

impl Eq for ZonedDateTime {}

impl Hash for ZonedDateTime {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.instant.hash(state);
        self.zone.hash(state);
    }
}

impl PartialOrd for ZonedDateTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ZonedDateTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}

impl fmt::Display for ZonedDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let offset = self.offset();
        let off = offset_id(&offset);
        write!(
            f,
            "{}{}",
            local_date_time_to_string(&self.to_local_date_time()),
            off
        )?;
        if !(self.zone.is_offset && self.zone.id == off) {
            write!(f, "[{}]", self.zone.id)?;
        }
        Ok(())
    }
}

pub fn parse_iso_local_date_time(text: &str) -> Option<NaiveDateTime> {
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"] {
        if let Ok(v) = NaiveDateTime::parse_from_str(text, fmt) {
            return Some(v);
        }
    }
    None
}

pub fn local_date_to_string(date: &NaiveDate) -> String {
    let year = date.year();
    if year.abs() < 10000 {
        if year < 0 {
            format!("-{:04}-{:02}-{:02}", -year, date.month(), date.day())
        } else {
            format!("{:04}-{:02}-{:02}", year, date.month(), date.day())
        }
    } else if year > 9999 {
        format!("+{}-{:02}-{:02}", year, date.month(), date.day())
    } else {
        format!("{}-{:02}-{:02}", year, date.month(), date.day())
    }
}

pub fn local_time_to_string(hour: u32, minute: u32, second: u32, nano: u32) -> String {
    if second == 0 && nano == 0 {
        return format!("{hour:02}:{minute:02}");
    }
    if nano == 0 {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else if nano.is_multiple_of(1_000_000) {
        format!("{hour:02}:{minute:02}:{second:02}.{:03}", nano / 1_000_000)
    } else if nano.is_multiple_of(1000) {
        format!("{hour:02}:{minute:02}:{second:02}.{:06}", nano / 1000)
    } else {
        format!("{hour:02}:{minute:02}:{second:02}.{nano:09}")
    }
}

pub fn local_date_time_to_string(dt: &NaiveDateTime) -> String {
    format!(
        "{}T{}",
        local_date_to_string(&dt.date()),
        local_time_to_string(
            dt.hour(),
            dt.minute(),
            dt.second(),
            dt.nanosecond() % 1_000_000_000
        )
    )
}

pub fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default()
}

pub fn local_date_of_epoch_day(days: i64) -> Option<NaiveDate> {
    epoch().checked_add_signed(Duration::try_days(days)?)
}

pub fn local_date_to_epoch_day(date: &NaiveDate) -> i64 {
    date.signed_duration_since(epoch()).num_days()
}

fn saturated_local_date_time(sign: i64) -> NaiveDateTime {
    if sign < 0 {
        NaiveDateTime::MIN
    } else {
        NaiveDateTime::MAX
    }
}

pub fn local_date_time_of_epoch_milli_utc(millis: i64) -> NaiveDateTime {
    DateTime::<Utc>::from_timestamp_millis(millis)
        .map_or_else(|| saturated_local_date_time(millis), |d| d.naive_utc())
}

pub(crate) fn local_date_time_of_epoch_micro_utc(micros: i64) -> NaiveDateTime {
    DateTime::<Utc>::from_timestamp_micros(micros)
        .map_or_else(|| saturated_local_date_time(micros), |d| d.naive_utc())
}

pub fn local_date_time_to_epoch_milli_utc(dt: &NaiveDateTime) -> i64 {
    dt.and_utc().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> DateTime<Utc> {
        text.parse::<NaiveDateTime>().unwrap().and_utc()
    }

    #[test]
    fn region_zones_apply_their_last_rules_after_2099() {
        let cases = [
            (
                "America/Los_Angeles",
                "2100-07-01T07:30:00",
                -25_200,
                "2100-07-01T14:30:00",
            ),
            (
                "America/Los_Angeles",
                "2100-12-01T12:00:00",
                -28_800,
                "2100-12-01T20:00:00",
            ),
            (
                "America/Los_Angeles",
                "2100-03-14T09:59:59",
                -28_800,
                "2100-03-14T16:59:59",
            ),
            (
                "America/Los_Angeles",
                "2100-03-14T10:00:00",
                -25_200,
                "2100-03-14T17:00:00",
            ),
            (
                "Europe/Berlin",
                "2200-07-01T12:00:00",
                7_200,
                "2200-07-01T10:00:00",
            ),
            (
                "Europe/Berlin",
                "2200-01-01T12:00:00",
                3_600,
                "2200-01-01T11:00:00",
            ),
            (
                "Australia/Lord_Howe",
                "2150-01-01T00:00:00",
                39_600,
                "2149-12-31T13:00:00",
            ),
            (
                "Australia/Lord_Howe",
                "2150-07-01T00:00:00",
                37_800,
                "2150-06-30T13:30:00",
            ),
            (
                "America/New_York",
                "9999-07-01T00:00:00",
                -14_400,
                "9999-07-01T04:00:00",
            ),
            (
                "Asia/Kolkata",
                "3000-07-01T00:00:00",
                19_800,
                "3000-06-30T18:30:00",
            ),
        ];
        for (id, instant, offset, local_to_utc) in cases {
            let zone = ZoneId::of(id).unwrap();
            let instant = utc(instant);
            assert_eq!(
                offset,
                zone.offset_at(&instant).local_minus_utc(),
                "{id} {instant}"
            );
            assert_eq!(
                utc(local_to_utc),
                zone.instant_of_local(&instant.naive_utc()),
                "{id} {instant}"
            );
        }
    }
}
