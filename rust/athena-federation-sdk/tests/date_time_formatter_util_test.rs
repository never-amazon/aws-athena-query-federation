use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};

use arrow::datatypes::TimeUnit;
use athena_federation_sdk::data::timestamp_tz;
use athena_federation_sdk::data::{DateTimeFormatterUtil, Value, ZoneId, ZonedDateTime};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};

static LOCK: Mutex<()> = Mutex::new(());

struct Guard<'a> {
    _lock: MutexGuard<'a, ()>,
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        DateTimeFormatterUtil::clear_timezone_packing_override();
    }
}

fn set_up() -> Guard<'static> {
    let lock = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    DateTimeFormatterUtil::enable_timezone_packing();
    Guard { _lock: lock }
}

fn utc() -> ZoneId {
    ZoneId::of("UTC").unwrap()
}

fn ldt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, nano: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_nano_opt(h, mi, s, nano)
        .unwrap()
}

fn minus_five() -> ZoneId {
    ZoneId::of("-05:00").unwrap()
}

const PACKED: i64 = 5_942_221_840_384_541;

#[test]
fn string_to_local_date_test() {
    let _g = set_up();
    let expected = NaiveDate::from_ymd_opt(2020, 2, 27).unwrap();
    assert_eq!(
        Some(expected),
        DateTimeFormatterUtil::string_to_local_date("27022020", Some("ddMMyyyy"), &utc())
    );
    assert_eq!(
        None,
        DateTimeFormatterUtil::string_to_local_date("2020-02-27", None, &utc())
    );
    assert_eq!(
        None,
        DateTimeFormatterUtil::string_to_local_date("27--02-2020", Some("ddMMyyyy"), &utc())
    );
}

#[test]
fn string_to_local_date_test_customer_configured_format_fail() {
    let _g = set_up();
    assert_eq!(
        None,
        DateTimeFormatterUtil::string_to_local_date("27--02-2020", Some("ddMMyyyy"), &utc())
    );
}

#[test]
fn string_to_zoned_date_time_test() {
    let _g = set_up();
    let local = ldt(2015, 12, 21, 17, 42, 34, 0);
    let expected = ZonedDateTime::of_local(local, minus_five());
    assert_eq!(
        Value::ZonedDateTime(expected),
        DateTimeFormatterUtil::string_to_zoned_date_time("2015-12-21T17:42:34-05:00", None, None)
    );
    assert_eq!(
        Value::LocalDateTime(local),
        DateTimeFormatterUtil::string_to_zoned_date_time(
            "2015-12-21T17:42:34",
            Some("yyyy-MM-dd'T'HH:mm:ss"),
            Some(&utc())
        )
    );
}

#[test]
fn string_tol_date_time_test() {
    let _g = set_up();
    let expected = ldt(2020, 2, 27, 0, 2, 27, 0);
    assert_eq!(
        Value::LocalDateTime(expected),
        DateTimeFormatterUtil::string_to_date_time(
            "00:02:27S2020-02-27",
            Some("HH:mm:ss'S'yyyy-MM-dd"),
            &utc()
        )
    );
    assert_eq!(
        Value::Null,
        DateTimeFormatterUtil::string_to_date_time("00:02:27S2020-02-27", None, &utc())
    );
}

#[test]
fn string_to_date_time_test_customer_configured_format_fail() {
    let _g = set_up();
    assert_eq!(
        Value::Null,
        DateTimeFormatterUtil::string_to_date_time(
            "00:02:27S2020---02-27",
            Some("HH:mm:ss'S'yyyy-MM-dd"),
            &utc()
        )
    );
}

#[test]
fn big_decimal_to_local_date_test() {
    let _g = set_up();
    let expected = NaiveDate::from_ymd_opt(2020, 2, 27).unwrap();
    let millis = expected
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    assert_eq!(
        Some(expected),
        DateTimeFormatterUtil::big_decimal_to_local_date(
            Some(&BigDecimal::from(millis)),
            Some(&utc())
        )
    );
    assert_eq!(
        None,
        DateTimeFormatterUtil::big_decimal_to_local_date(None, None)
    );
}

#[test]
fn big_decimal_to_local_date_time_test() {
    let _g = set_up();
    let expected = ldt(2020, 2, 27, 0, 2, 27, 0);
    let millis = expected.and_utc().timestamp_millis();
    assert_eq!(
        Some(expected),
        DateTimeFormatterUtil::big_decimal_to_local_date_time(
            Some(&BigDecimal::from_str(&millis.to_string()).unwrap()),
            Some(&utc())
        )
    );
    assert_eq!(
        None,
        DateTimeFormatterUtil::big_decimal_to_local_date_time(None, None)
    );
}

#[test]
fn infer_date_time_format_test() {
    let _g = set_up();
    assert_eq!(
        Some("yyyy-MM-dd"),
        DateTimeFormatterUtil::infer_date_time_format("2020-02-27")
    );
    assert_eq!(
        Some("yyyy-MM-dd'T'HH:mm:ss"),
        DateTimeFormatterUtil::infer_date_time_format("2020-02-27T00:02:27")
    );
    assert_eq!(
        Some("yyyy-MM-dd'T'HH:mm:ssZZ"),
        DateTimeFormatterUtil::infer_date_time_format("2020-02-27T00:02:27Z")
    );
    assert_eq!(
        Some("yyyy-MM-dd'T'HH:mm:ssZZ"),
        DateTimeFormatterUtil::infer_date_time_format("2020-02-27T00:02:27-05:00")
    );
    assert_eq!(
        Some("yyyyMMdd'T'HHmmss"),
        DateTimeFormatterUtil::infer_date_time_format("20200227T000227")
    );
    assert_eq!(
        Some("yyyyMMdd'T'HHmmssZZ"),
        DateTimeFormatterUtil::infer_date_time_format("20200227T000227Z")
    );
    assert_eq!(
        None,
        DateTimeFormatterUtil::infer_date_time_format("2020202020202020202020")
    );
}

#[test]
fn timestamp_milli_tz_holder_from_object() {
    let _g = set_up();
    let arrow_type = timestamp_tz(TimeUnit::Millisecond, "-05:00");
    let expected_zdt = ZonedDateTime::of_local(ldt(2015, 12, 21, 17, 42, 34, 0), minus_five());
    let expected_unpacked = expected_zdt.to_epoch_milli();
    let zdt_value = Value::ZonedDateTime(expected_zdt.clone());
    assert_eq!(
        PACKED,
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(&zdt_value, None)
            .unwrap()
            .value
    );
    assert_eq!(
        expected_zdt,
        DateTimeFormatterUtil::construct_zoned_date_time(PACKED, &arrow_type).unwrap()
    );
    DateTimeFormatterUtil::disable_timezone_packing();
    let holder =
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(&zdt_value, None).unwrap();
    assert_eq!(expected_unpacked, holder.value);
    assert_eq!("-05:00", holder.timezone);
    assert_ne!(PACKED, holder.value);
    assert_eq!(
        expected_zdt,
        DateTimeFormatterUtil::construct_zoned_date_time(expected_unpacked, &arrow_type).unwrap()
    );
}

#[test]
fn timezone_packing_override_takes_precedence() {
    let _g = set_up();
    let zdt = ZonedDateTime::of_local(ldt(2015, 12, 21, 17, 42, 34, 0), minus_five());
    let unpacked = zdt.to_epoch_milli();
    let v = Value::ZonedDateTime(zdt);
    DateTimeFormatterUtil::override_timezone_packing(false);
    assert_eq!(
        unpacked,
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(&v, None)
            .unwrap()
            .value
    );
    DateTimeFormatterUtil::disable_timezone_packing();
    DateTimeFormatterUtil::override_timezone_packing(true);
    assert_eq!(
        PACKED,
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(&v, None)
            .unwrap()
            .value
    );
}

#[test]
fn clear_timezone_packing_override_restores_default() {
    let _g = set_up();
    let zdt = ZonedDateTime::of_local(ldt(2015, 12, 21, 17, 42, 34, 0), minus_five());
    DateTimeFormatterUtil::override_timezone_packing(false);
    DateTimeFormatterUtil::clear_timezone_packing_override();
    assert_eq!(
        PACKED,
        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(
            &Value::ZonedDateTime(zdt),
            None
        )
        .unwrap()
        .value
    );
}

#[test]
fn timestamp_micro_tz_holder_from_object() {
    let _g = set_up();
    let expected_micros: i64 = 2_134_123_412_348_117 + 5 * 3_600_000_000;
    let expected_instant = DateTime::<Utc>::UNIX_EPOCH + Duration::microseconds(expected_micros);
    let local = ldt(2037, 8, 17, 12, 3, 32, 348_117 * 1000);
    let expected_zdt = ZonedDateTime::of_local(local, minus_five());
    assert_eq!(expected_instant, expected_zdt.to_instant());
    let v = Value::ZonedDateTime(expected_zdt.clone());
    assert!(
        DateTimeFormatterUtil::timestamp_micro_tz_holder_from_object(&v, None).is_err(),
        "Exception was expected since packing should not work for microseconds"
    );
    DateTimeFormatterUtil::disable_timezone_packing();
    let holder = DateTimeFormatterUtil::timestamp_micro_tz_holder_from_object(&v, None).unwrap();
    assert_eq!(expected_micros, holder.value);
    assert_eq!("-05:00", holder.timezone);
    let arrow_type = timestamp_tz(TimeUnit::Microsecond, "-05:00");
    assert_eq!(
        expected_zdt,
        DateTimeFormatterUtil::construct_zoned_date_time(expected_micros, &arrow_type).unwrap()
    );
}

#[test]
fn date_time_packing_round_trip() {
    let _g = set_up();
    let utc_offset = ZoneId::of("Z").unwrap();
    let cases = vec![
        ZonedDateTime::of_epoch_milli(10, utc_offset.clone()).unwrap(),
        ZonedDateTime::of_local(ldt(2015, 12, 21, 17, 42, 34, 0), utc_offset),
        ZonedDateTime::of_epoch_milli(45_423_958_493, ZoneId::of("-11:00").unwrap()).unwrap(),
    ];
    for zdt in cases {
        let holder = DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(
            &Value::ZonedDateTime(zdt.clone()),
            None,
        )
        .unwrap();
        let arrow_type = timestamp_tz(TimeUnit::Millisecond, &holder.timezone);
        let back =
            DateTimeFormatterUtil::construct_zoned_date_time(holder.value, &arrow_type).unwrap();
        assert_eq!(zdt.offset(), back.offset());
        assert_eq!(zdt.to_instant(), back.to_instant());
    }
}
