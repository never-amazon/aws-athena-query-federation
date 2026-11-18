mod common_field_writer;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use arrow::datatypes::{DataType, TimeUnit};

use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, ScalarFieldWriter};
use athena_federation_sdk::data::{BlockAllocatorImpl, FieldVector, Value};
use athena_federation_sdk::domain::predicate::{ConstraintProjector, Range, SortedRangeSet};

use common_field_writer::{new_vector, CallCounter, MockConstraintProjector};

const EXPECTED: i64 = 1_672_531_200_000_i64;

fn expected() -> i64 {
    EXPECTED
}

struct Fixture {
    vector: FieldVector,
    extractor_calls: CallCounter,
    projector: Arc<MockConstraintProjector>,
}

fn configure_extractor(value: i64, is_set: i32, calls: &CallCounter) -> Extractor<()> {
    let counter = calls.0.clone();
    Extractor::DateMilli(Arc::new(move |_ctx: &()| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok((is_set > 0).then_some(value))
    }))
}

fn set_up() -> Fixture {
    Fixture {
        vector: new_vector("testVector", DataType::Date64),
        extractor_calls: CallCounter::new(),
        projector: MockConstraintProjector::new(),
    }
}

fn writer(fx: &Fixture, value: i64, is_set: i32, with_constraint: bool) -> ScalarFieldWriter<()> {
    let constraint: Option<ConstraintProjector> = if with_constraint {
        Some(fx.projector.as_projector())
    } else {
        None
    };
    ScalarFieldWriter::new(
        configure_extractor(value, is_set, &fx.extractor_calls),
        constraint,
    )
}

fn verify_assertions(fx: &Fixture, expected_result: bool, actual_result: bool) {
    assert_eq!(expected_result, actual_result);
    assert_eq!(fx.vector.get_raw(0), Value::Long(expected()));
}

#[test]
fn write_with_valid_date_milli_value_should_write_successfully() {
    let mut fx = set_up();
    fx.projector.when_apply(
        Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc(),
        ),
        true,
    );
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, true, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector.times_called_with(&Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc()
        )),
        1
    );
}

#[test]
fn write_with_constraint_failure_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(
        Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc(),
        ),
        false,
    );
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, false, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector.times_called_with(&Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc()
        )),
        1
    );
}

#[test]
fn write_without_constraints_should_write_successfully() {
    let mut fx = set_up();
    let mut w = writer(&fx, expected(), 1, false);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, true, result);
    assert_eq!(fx.extractor_calls.count(), 1);
}

#[test]
fn write_with_null_date_milli_value_should_mark_vector_as_null() {
    let mut fx = set_up();
    let mut w = writer(&fx, 0i64, 0, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
}

#[test]
fn write_with_non_zero_value_marked_null_should_mark_as_null() {
    let mut fx = set_up();
    let mut w = writer(&fx, expected(), 0, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
}

const NEGATIVE_INFINITY_MILLIS: i64 = -9_223_372_036_832_400_000;
const POSITIVE_INFINITY_MILLIS: i64 = 9_223_372_036_825_200_000;

fn range_projector(range: Range) -> ConstraintProjector {
    let set = SortedRangeSet::of_ranges(range, Vec::new()).unwrap();
    Arc::new(move |v: &Value| set.contains_value(v))
}

#[test]
fn write_with_out_of_range_millis_applies_range_constraints_like_java() {
    let a = BlockAllocatorImpl::new();
    let t = DataType::Date64;
    let v = chrono::NaiveDate::from_ymd_opt(2020, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let cases = [
        ("lt", Range::less_than(&a, &t, v).unwrap(), true, false),
        (
            "le",
            Range::less_than_or_equal(&a, &t, v).unwrap(),
            true,
            false,
        ),
        ("gt", Range::greater_than(&a, &t, v).unwrap(), false, true),
        (
            "ge",
            Range::greater_than_or_equal(&a, &t, v).unwrap(),
            false,
            true,
        ),
    ];
    for (name, range, negative, positive) in cases {
        for (millis, expected_result) in [
            (NEGATIVE_INFINITY_MILLIS, negative),
            (POSITIVE_INFINITY_MILLIS, positive),
        ] {
            let mut vector = new_vector("testVector", DataType::Date64);
            let calls = CallCounter::new();
            let mut w = ScalarFieldWriter::new(
                configure_extractor(millis, 1, &calls),
                Some(range_projector(range.clone())),
            );
            let result = w.write(&mut vector, &(), 0).unwrap();
            assert_eq!(result, expected_result, "{name} {millis}");
            assert_eq!(vector.get_raw(0), Value::Long(millis));
        }
    }
}

#[test]
fn get_object_returns_a_value_for_millis_outside_the_chrono_range() {
    let mut vector = new_vector("testVector", DataType::Date64);
    vector
        .set(0, &Value::Long(NEGATIVE_INFINITY_MILLIS))
        .unwrap();
    vector
        .set(1, &Value::Long(POSITIVE_INFINITY_MILLIS))
        .unwrap();
    assert_eq!(
        vector.get_object(0),
        Value::LocalDateTime(chrono::NaiveDateTime::MIN)
    );
    assert_eq!(
        vector.get_object(1),
        Value::LocalDateTime(chrono::NaiveDateTime::MAX)
    );
}

#[test]
fn get_object_returns_a_value_for_timestamps_outside_the_chrono_range() {
    for (data_type, raw) in [
        (DataType::Timestamp(TimeUnit::Second, None), i64::MIN),
        (DataType::Timestamp(TimeUnit::Millisecond, None), i64::MIN),
        (DataType::Timestamp(TimeUnit::Microsecond, None), i64::MIN),
    ] {
        let mut vector = new_vector("testVector", data_type.clone());
        vector.set(0, &Value::Long(raw)).unwrap();
        vector.set(1, &Value::Long(i64::MAX)).unwrap();
        assert_eq!(
            vector.get_object(0),
            Value::LocalDateTime(chrono::NaiveDateTime::MIN),
            "{data_type}"
        );
        assert_eq!(
            vector.get_object(1),
            Value::LocalDateTime(chrono::NaiveDateTime::MAX),
            "{data_type}"
        );
    }
}

#[test]
fn write_with_constraint_failure_despite_is_set_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(
        Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc(),
        ),
        false,
    );
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(!fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector.times_called_with(&Value::LocalDateTime(
            chrono::DateTime::from_timestamp_millis(expected())
                .unwrap()
                .naive_utc()
        )),
        1
    );
}
