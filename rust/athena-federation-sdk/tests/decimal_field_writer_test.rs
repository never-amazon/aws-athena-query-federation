mod common_field_writer;

use std::any::Any;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::str::FromStr;

use arrow::datatypes::DataType;
use bigdecimal::BigDecimal;
use athena_federation_sdk::data::writers::extractors::DecimalExtractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, DecimalFieldWriter};
use athena_federation_sdk::data::writers::holders::NullableDecimalHolder;
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::domain::predicate::ConstraintProjector;
use athena_federation_sdk::FederationError;
use common_field_writer::{new_vector, CallCounter, MockConstraintProjector};

fn expected() -> BigDecimal {
    BigDecimal::from_str("123.456").unwrap()
}

struct Fixture {
    vector: FieldVector,
    extractor_calls: CallCounter,
    projector: Arc<MockConstraintProjector>,
}

fn configure_extractor(value: BigDecimal, is_set: i32, calls: &CallCounter) -> Arc<dyn DecimalExtractor> {
    let counter = calls.0.clone();
    Arc::new(move |_ctx: &dyn Any, holder: &mut NullableDecimalHolder| {
        counter.fetch_add(1, Ordering::SeqCst);
        holder.is_set = is_set;
        holder.value = value.clone();
        Ok(())
    })
}

fn set_up() -> Fixture {
    Fixture {
        vector: new_vector("testVector", DataType::Decimal128(10, 2)),
        extractor_calls: CallCounter::new(),
        projector: MockConstraintProjector::new(),
    }
}

fn writer(fx: &Fixture, value: BigDecimal, is_set: i32, with_constraint: bool) -> DecimalFieldWriter {
    let constraint: Option<Arc<dyn ConstraintProjector>> = if with_constraint { Some(fx.projector.clone()) } else { None };
    DecimalFieldWriter::new(configure_extractor(value, is_set, &fx.extractor_calls), constraint)
}

fn verify_assertions(fx: &Fixture, expected_result: bool, actual_result: bool) {
    assert_eq!(expected_result, actual_result);
    assert_eq!(fx.vector.get_raw(0), Value::Decimal(BigDecimal::from_str("123.46").unwrap()));
}

#[test]
fn write_with_valid_decimal_value_should_write_successfully() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Decimal(expected().clone()), true);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, true, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Decimal(expected().clone())), 1);
}

#[test]
fn write_with_constraint_failure_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Decimal(expected().clone()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, false, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Decimal(expected().clone())), 1);
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
fn write_with_null_decimal_value_should_mark_vector_as_null() {
    let mut fx = set_up();
    let mut w = writer(&fx, BigDecimal::from(0), 0, true);
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

#[test]
fn write_with_invalid_decimal_value_should_throw_exception() {
    let mut fx = set_up();
    let invalid_value = BigDecimal::from_str("1234567890.1234567890").unwrap();
    fx.projector.when_apply(Value::Decimal(invalid_value.clone()), true);
    let mut w = writer(&fx, invalid_value, 1, true);
    match w.write(&mut fx.vector, &(), 0) {
        Err(FederationError::UnsupportedOperation(msg)) => assert!(msg.contains("BigDecimal precision cannot be greater than")),
        other => panic!("Expected UnsupportedOperation but got {:?}", other),
    }
    assert_eq!(fx.extractor_calls.count(), 1);
}

#[test]
fn write_with_constraint_failure_despite_is_set_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Decimal(expected().clone()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(!fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Decimal(expected().clone())), 1);
}
