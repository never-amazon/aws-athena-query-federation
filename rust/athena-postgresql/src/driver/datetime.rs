use chrono::{DateTime, NaiveDate, NaiveDateTime};

pub(crate) const DATE_POSITIVE_INFINITY: i64 = 9_223_372_036_825_200_000;
pub(crate) const DATE_NEGATIVE_INFINITY: i64 = -9_223_372_036_832_400_000;

const MILLIS_PER_DAY: i64 = 86_400_000;
const UNIX_EPOCH_JDN: i64 = 2_440_588;
const GREGORIAN_CUTOVER_DAY: i64 = -141_427;
const MAX_FRACTION_DIGITS: usize = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParsedTimestamp {
    pub(crate) bc: bool,
    pub(crate) year: i64,
    pub(crate) month: i64,
    pub(crate) day: i64,
    pub(crate) hour: i64,
    pub(crate) minute: i64,
    pub(crate) second: i64,
    pub(crate) nanos: u32,
    pub(crate) offset_seconds: i64,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn digits(&mut self) -> Option<&[u8]> {
        let start = self.pos;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
        (self.pos > start).then(|| &self.bytes[start..self.pos])
    }

    fn number(&mut self) -> Option<i64> {
        self.digits()?
            .iter()
            .try_fold(0i32, |n, d| {
                n.checked_mul(10)?.checked_add(i32::from(d - b'0'))
            })
            .map(i64::from)
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
}

pub(crate) fn parse_backend_timestamp(text: &str) -> Option<ParsedTimestamp> {
    let mut c = Cursor {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let mut parsed = ParsedTimestamp {
        bc: false,
        year: 1970,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
        nanos: 0,
        offset_seconds: 0,
    };
    c.skip_whitespace();
    let mut has_date = false;
    let mut has_time = false;
    let start = c.pos;
    let first = c.number();
    if c.peek() == Some(b'-') && first.is_some() {
        has_date = true;
        parsed.year = first?;
        c.pos += 1;
        parsed.month = c.number()?;
        if !c.eat(b'-') {
            return None;
        }
        parsed.day = c.number()?;
        c.skip_whitespace();
    } else {
        c.pos = start;
    }
    if c.peek().is_some_and(|b| b.is_ascii_digit()) {
        has_time = true;
        parsed.hour = c.number()?;
        if !c.eat(b':') {
            return None;
        }
        parsed.minute = c.number()?;
        if !c.eat(b':') {
            return None;
        }
        parsed.second = c.number()?;
        if c.eat(b'.') {
            let digits = c.digits()?;
            if digits.len() > MAX_FRACTION_DIGITS {
                return None;
            }
            let value = digits
                .iter()
                .fold(0u32, |n, d| n * 10 + u32::from(d - b'0'));
            parsed.nanos =
                value * 10u32.pow(u32::try_from(MAX_FRACTION_DIGITS - digits.len()).ok()?);
        }
        c.skip_whitespace();
    }
    if let Some(sign @ (b'+' | b'-')) = c.peek() {
        c.pos += 1;
        let hours = c.number()?;
        let minutes = if c.eat(b':') { c.number()? } else { 0 };
        let seconds = if c.eat(b':') { c.number()? } else { 0 };
        if hours > 18 || minutes > 59 || seconds > 59 {
            return None;
        }
        let total = hours * 3600 + minutes * 60 + seconds;
        if total > 18 * 3600 {
            return None;
        }
        parsed.offset_seconds = if sign == b'-' { -total } else { total };
        c.skip_whitespace();
    }
    if has_date {
        if c.bytes[c.pos..].starts_with(b"AD") {
            c.pos += 2;
        } else if c.bytes[c.pos..].starts_with(b"BC") {
            parsed.bc = true;
            c.pos += 2;
        }
    }
    (c.pos == c.bytes.len() && (has_date || has_time)).then_some(parsed)
}

fn jdn_offset(year: i64, month: i64) -> (i64, i64) {
    let a = (14 - month).div_euclid(12);
    (year + 4800 - a, month + 12 * a - 3)
}

fn gregorian_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let (y, m) = jdn_offset(year, month);
    day + (153 * m + 2).div_euclid(5) + 365 * y + y.div_euclid(4) - y.div_euclid(100)
        + y.div_euclid(400)
        - 32_045
        - UNIX_EPOCH_JDN
}

fn julian_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let (y, m) = jdn_offset(year, month);
    day + (153 * m + 2).div_euclid(5) + 365 * y + y.div_euclid(4) - 32_083 - UNIX_EPOCH_JDN
}

fn julian_fields(epoch_day: i64) -> (i64, i64, i64) {
    julian_like_fields(epoch_day + UNIX_EPOCH_JDN + 32_082)
}

fn gregorian_fields(epoch_day: i64) -> (i64, i64, i64) {
    let shifted = epoch_day + UNIX_EPOCH_JDN + 32_044;
    let centuries = (4 * shifted + 3).div_euclid(146_097);
    let in_century = shifted - (146_097 * centuries).div_euclid(4);
    let (year, month, day) = julian_like_fields(in_century);
    (year + 100 * centuries, month, day)
}

fn julian_like_fields(c: i64) -> (i64, i64, i64) {
    let d = (4 * c + 3).div_euclid(1461);
    let e = c - (1461 * d).div_euclid(4);
    let m = (5 * e + 2).div_euclid(153);
    let day = e - (153 * m + 2).div_euclid(5) + 1;
    let month = m + 3 - 12 * m.div_euclid(10);
    let year = d - 4800 + m.div_euclid(10);
    (year, month, day)
}

pub(crate) fn hybrid_epoch_day(year: i64, month: i64, day: i64) -> i64 {
    let gregorian = gregorian_epoch_day(year, month, day);
    if gregorian >= GREGORIAN_CUTOVER_DAY {
        gregorian
    } else {
        julian_epoch_day(year, month, day)
    }
}

fn hybrid_fields(epoch_day: i64) -> (i64, i64, i64) {
    if epoch_day >= GREGORIAN_CUTOVER_DAY {
        gregorian_fields(epoch_day)
    } else {
        julian_fields(epoch_day)
    }
}

impl ParsedTimestamp {
    fn proleptic_year(&self) -> i64 {
        if self.bc {
            1 - self.year
        } else {
            self.year
        }
    }

    fn millis_on_day(&self, day: i64) -> Option<i64> {
        let seconds = self.hour * 3600 + self.minute * 60 + self.second - self.offset_seconds;
        day.checked_mul(MILLIS_PER_DAY)?
            .checked_add(seconds.checked_mul(1000)?)?
            .checked_add(i64::from(self.nanos / 1_000_000))
    }

    pub(crate) fn java_millis(&self) -> Option<i64> {
        self.millis_on_day(hybrid_epoch_day(
            self.proleptic_year(),
            self.month,
            self.day,
        ))
    }

    pub(crate) fn epoch_millis(&self) -> Option<i64> {
        self.millis_on_day(gregorian_epoch_day(
            self.proleptic_year(),
            self.month,
            self.day,
        ))
    }

    pub(crate) fn java_date_millis(&self) -> Option<i64> {
        let day = hybrid_epoch_day(self.proleptic_year(), self.month, self.day);
        day.checked_mul(MILLIS_PER_DAY)
    }
}

pub(crate) fn millis_to_naive(millis: i64, nanos: u32) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(millis.div_euclid(1000), nanos).map(|dt| dt.naive_utc())
}

pub(crate) fn java_sql_date_local_date(millis: i64) -> Option<NaiveDate> {
    let (year, month, day) = hybrid_fields(millis.div_euclid(MILLIS_PER_DAY));
    let year_of_era = if year <= 0 { 1 - year } else { year };
    NaiveDate::from_ymd_opt(
        i32::try_from(year_of_era % 10_000).ok()?,
        u32::try_from(month).ok()?,
        u32::try_from(day).ok()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch_day(date: NaiveDate) -> i64 {
        date.signed_duration_since(NaiveDate::default()).num_days()
    }

    fn millis(text: &str) -> i64 {
        parse_backend_timestamp(text)
            .unwrap()
            .java_millis()
            .unwrap()
    }

    #[test]
    fn calendars_agree_with_known_epoch_days() {
        assert_eq!(gregorian_epoch_day(1970, 1, 1), 0);
        assert_eq!(gregorian_epoch_day(1582, 10, 15), GREGORIAN_CUTOVER_DAY);
        assert_eq!(julian_epoch_day(1582, 10, 5), GREGORIAN_CUTOVER_DAY);
        for day in [-800_000, -141_428, -141_427, 0, 2_932_896] {
            let (y, m, d) = hybrid_fields(day);
            assert_eq!(hybrid_epoch_day(y, m, d), day);
        }
    }

    #[test]
    fn old_timestamps_use_the_julian_calendar_like_java_sql_timestamp() {
        let expected = |y, m, d| {
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis()
        };
        assert_eq!(millis("1000-01-01 12:00:00"), expected(1000, 1, 6));
        assert_eq!(millis("0044-03-15 12:00:00 BC"), expected(-43, 3, 13));
        assert_eq!(millis("1582-10-15 12:00:00"), expected(1582, 10, 15));
        assert_eq!(millis("1582-10-04 12:00:00"), expected(1582, 10, 14));
        assert_eq!(millis("10000-01-01 12:00:00"), expected(10000, 1, 1));
        assert_eq!(
            millis("2020-06-01 12:00:00.987654+05:30"),
            expected(2020, 6, 1) - 19_800_000 + 987
        );
        assert_eq!(
            millis("1900-01-01 00:00:00-04:56:02"),
            expected(1900, 1, 1) - 43_200_000 + 17_762_000
        );
    }

    #[test]
    fn trailing_junk_and_bad_fields_are_rejected() {
        assert!(parse_backend_timestamp("2020-01-01 00:00:00 XY").is_none());
        assert!(parse_backend_timestamp("2020-01-01T00:00:00").is_none());
        assert!(parse_backend_timestamp("").is_none());
        assert!(parse_backend_timestamp("2020-01-01 00:00:00.1234567890").is_none());
        assert!(parse_backend_timestamp("0001-01-01 BC").unwrap().bc);
    }

    #[test]
    fn fields_outside_the_java_int_range_are_rejected_without_overflow() {
        for text in [
            "2020-01-01 00:00:00+9999999999999999",
            "2020-01-01 00:00:00-9999999999999999:00",
            "2020-01-01 00:00:00+00:9999999999999999",
            "9223372036854775807-01-01 00:00:00",
            "2147483648-01-01 00:00:00",
            "2020-9223372036854775807-01 00:00:00",
            "2020-01-9223372036854775807 00:00:00",
            "2020-01-01 2562047788015216:00:00",
            "2020-01-01 00:9223372036854775807:00",
            "2020-01-01 00:00:9223372036854775807",
        ] {
            let parsed = parse_backend_timestamp(text);
            assert!(
                parsed.is_none_or(|p| p.java_millis().is_none() && p.java_date_millis().is_none()),
                "{text}"
            );
        }
    }

    #[test]
    fn fields_inside_the_java_int_range_do_not_overflow() {
        for text in [
            "2147483647-12-31 2147483647:2147483647:2147483647",
            "2147483647-2147483647-2147483647 00:00:00 BC",
        ] {
            let parsed = parse_backend_timestamp(text).unwrap();
            assert!(parsed.java_millis().is_none(), "{text}");
            assert!(parsed.java_date_millis().is_none(), "{text}");
        }
    }

    #[test]
    fn java_sql_date_strings_keep_four_year_digits_and_drop_the_era() {
        let day = |text: &str| {
            epoch_day(
                java_sql_date_local_date(
                    parse_backend_timestamp(text)
                        .unwrap()
                        .java_date_millis()
                        .unwrap(),
                )
                .unwrap(),
            )
        };
        assert_eq!(day("0001-01-01 BC"), -719_162);
        assert_eq!(day("10000-01-01"), -719_528);
        assert_eq!(day("1582-10-04"), -141_438);
        assert_eq!(
            day("1000-01-01"),
            epoch_day(NaiveDate::from_ymd_opt(1000, 1, 1).unwrap())
        );
        assert_eq!(
            epoch_day(java_sql_date_local_date(DATE_POSITIVE_INFINITY).unwrap()),
            2_565_691
        );
        assert_eq!(
            epoch_day(java_sql_date_local_date(DATE_NEGATIVE_INFINITY).unwrap()),
            2_588_078
        );
    }
}
