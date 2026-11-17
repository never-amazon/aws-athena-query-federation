use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Duration, FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike, Utc};
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
        4 => (digits[0..2].parse::<i32>().ok()?, digits[2..4].parse::<i32>().ok()?, 0),
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
        format!("{}{:02}:{:02}:{:02}", sign, h, m, s)
    } else {
        format!("{}{:02}:{:02}", sign, h, m)
    }
}

impl ZoneId {
    pub fn of(id: &str) -> Result<ZoneId> {
        if id.is_empty() {
            return Err(FederationError::illegal_argument("Invalid ID for ZoneId, empty"));
        }
        if id.len() <= 1 || id.starts_with('+') || id.starts_with('-') {
            let offset = parse_offset_id(id)
                .ok_or_else(|| FederationError::illegal_argument(format!("Invalid ID for ZoneOffset, invalid format: {}", id)))?;
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
                        FederationError::illegal_argument(format!("Invalid ID for offset-based ZoneId: {}", id))
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
            .map_err(|_| FederationError::Runtime(format!("Unknown time-zone ID: {}", id)))?;
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

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub fn is_offset(&self) -> bool {
        self.is_offset
    }

    pub fn offset_at(&self, instant: &DateTime<Utc>) -> FixedOffset {
        match self.rules {
            ZoneRules::Fixed(off) => off,
            ZoneRules::Region(tz) => tz.offset_from_utc_datetime(&instant.naive_utc()).fix(),
        }
    }

    pub fn instant_of_local(&self, local: &NaiveDateTime) -> DateTime<Utc> {
        match self.rules {
            ZoneRules::Fixed(off) => Utc.from_utc_datetime(&(*local - Duration::seconds(off.local_minus_utc() as i64))),
            ZoneRules::Region(tz) => match tz.from_local_datetime(local) {
                LocalResult::Single(dt) => dt.with_timezone(&Utc),
                LocalResult::Ambiguous(early, _) => early.with_timezone(&Utc),
                LocalResult::None => {
                    let before = tz
                        .offset_from_utc_datetime(&(*local - Duration::days(1)))
                        .fix();
                    Utc.from_utc_datetime(&(*local - Duration::seconds(before.local_minus_utc() as i64)))
                }
            },
        }
    }
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
        let instant = DateTime::<Utc>::from_timestamp_millis(millis)
            .ok_or_else(|| FederationError::Arithmetic(format!("Instant exceeds minimum or maximum instant: {}", millis)))?;
        Ok(ZonedDateTime { instant, zone })
    }

    pub fn parse(text: &str) -> Result<ZonedDateTime> {
        let err = || FederationError::Runtime(format!("Text '{}' could not be parsed", text));
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
        let off_idx = after_t
            .find(['Z', '+', '-'])
            .ok_or_else(err)?;
        let local_text = &main[..t_idx + 1 + off_idx];
        let offset_text = &after_t[off_idx..];
        let local = parse_iso_local_date_time(local_text).ok_or_else(err)?;
        let offset = parse_offset_id(offset_text).ok_or_else(err)?;
        let instant = Utc.from_utc_datetime(&(local - Duration::seconds(offset.local_minus_utc() as i64)));
        let zone = match bracket {
            Some(id) => ZoneId::of(id).map_err(|_| err())?,
            None => ZoneId::of_offset(offset),
        };
        if bracket.is_some() && zone.offset_at(&instant) != offset {
            return Ok(ZonedDateTime::of_local(local, zone));
        }
        Ok(ZonedDateTime { instant, zone })
    }

    pub fn get_zone(&self) -> &ZoneId {
        &self.zone
    }

    pub fn get_offset(&self) -> FixedOffset {
        self.zone.offset_at(&self.instant)
    }

    pub fn to_instant(&self) -> DateTime<Utc> {
        self.instant
    }

    pub fn to_epoch_milli(&self) -> i64 {
        self.instant.timestamp_millis()
    }

    pub fn get_epoch_second(&self) -> i64 {
        self.instant.timestamp()
    }

    pub fn get_nano(&self) -> u32 {
        self.instant.timestamp_subsec_nanos()
    }

    pub fn to_local_date_time(&self) -> NaiveDateTime {
        self.instant.naive_utc() + Duration::seconds(self.get_offset().local_minus_utc() as i64)
    }

    pub fn to_local_date(&self) -> NaiveDate {
        self.to_local_date_time().date()
    }

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
            .then(self.get_nano().cmp(&other.get_nano()))
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
        let offset = self.get_offset();
        let off = offset_id(&offset);
        write!(f, "{}{}", local_date_time_to_string(&self.to_local_date_time()), off)?;
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
    let mut s = format!("{:02}:{:02}", hour, minute);
    if second > 0 || nano > 0 {
        s.push_str(&format!(":{:02}", second));
        if nano > 0 {
            if nano.is_multiple_of(1_000_000) {
                s.push_str(&format!(".{:03}", nano / 1_000_000));
            } else if nano.is_multiple_of(1000) {
                s.push_str(&format!(".{:06}", nano / 1000));
            } else {
                s.push_str(&format!(".{:09}", nano));
            }
        }
    }
    s
}

pub fn local_date_time_to_string(dt: &NaiveDateTime) -> String {
    format!(
        "{}T{}",
        local_date_to_string(&dt.date()),
        local_time_to_string(dt.hour(), dt.minute(), dt.second(), dt.nanosecond() % 1_000_000_000)
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

pub fn local_date_time_of_epoch_milli_utc(millis: i64) -> Option<NaiveDateTime> {
    DateTime::<Utc>::from_timestamp_millis(millis).map(|d| d.naive_utc())
}

pub fn local_date_time_to_epoch_milli_utc(dt: &NaiveDateTime) -> i64 {
    dt.and_utc().timestamp_millis()
}
