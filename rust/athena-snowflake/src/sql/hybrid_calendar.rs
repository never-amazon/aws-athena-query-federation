use chrono::{Datelike, Days, NaiveDate, NaiveDateTime, TimeDelta, Timelike};

const MILLIS_PER_DAY: i64 = 86_400_000;
const MILLIS_PER_DAY_I32: i32 = 86_400_000;
const ADJUSTMENT_THRESHOLD_MILLIS: i64 = -12_220_156_800_000;
const UNIX_EPOCH_JULIAN_DAY: i64 = 2_440_588;
const GAP_DAYS: u64 = 10;

fn gregorian_gap() -> Option<(NaiveDate, NaiveDate)> {
    Some((
        NaiveDate::from_ymd_opt(1582, 10, 5)?,
        NaiveDate::from_ymd_opt(1582, 10, 14)?,
    ))
}

pub(crate) enum DateLabel {
    Same,
    Shifted(NaiveDate),
    Unparseable(String),
}

pub(crate) fn java_sql_date_label(date: NaiveDate) -> DateLabel {
    if let Some((gap_start, gap_end)) = gregorian_gap() {
        if (gap_start..=gap_end).contains(&date) {
            return date
                .checked_add_days(Days::new(GAP_DAYS))
                .map_or(DateLabel::Same, DateLabel::Shifted);
        }
    }
    if date.year() > 0 {
        return DateLabel::Same;
    }
    let era_year = 1 - date.year();
    match NaiveDate::from_ymd_opt(era_year, date.month(), date.day()) {
        Some(label) => DateLabel::Shifted(label),
        None => DateLabel::Unparseable(format!(
            "{era_year:04}-{:02}-{:02}",
            date.month(),
            date.day()
        )),
    }
}

fn julian_calendar_fields(epoch_day: i64) -> (i64, u32, u32) {
    let c = epoch_day + UNIX_EPOCH_JULIAN_DAY + 32_082;
    let d = (4 * c + 3).div_euclid(1461);
    let e = c - (1461 * d).div_euclid(4);
    let m = (5 * e + 2).div_euclid(153);
    let day = e - (153 * m + 2).div_euclid(5) + 1;
    let month = m + 3 - 12 * m.div_euclid(10);
    let year = d - 4800 + m.div_euclid(10);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

fn julian_to_gregorian_millis(epoch_millis: i64) -> i64 {
    if epoch_millis >= ADJUSTMENT_THRESHOLD_MILLIS {
        return 0;
    }
    let (year, month, day) = julian_calendar_fields(epoch_millis.div_euclid(MILLIS_PER_DAY));
    let mut era_year = if year > 0 { year } else { 1 - year };
    if month == 1 || (month == 2 && day <= 28) {
        era_year -= 1;
    }
    let hundreds = i32::try_from(era_year / 100).unwrap_or(i32::MAX);
    let days = hundreds - hundreds / 4 - 2;
    i64::from(days.wrapping_mul(MILLIS_PER_DAY_I32))
}

pub(crate) fn java_sql_timestamp(timestamp: NaiveDateTime) -> NaiveDateTime {
    let millis = julian_to_gregorian_millis(timestamp.and_utc().timestamp_millis());
    if millis == 0 {
        return timestamp;
    }
    timestamp
        .checked_add_signed(TimeDelta::milliseconds(millis))
        .and_then(|adjusted| adjusted.with_nanosecond(timestamp.nanosecond()))
        .unwrap_or(timestamp)
}
