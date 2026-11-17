mod common_field_writer;

use std::any::Any;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use arrow::datatypes::DataType;

use athena_federation_sdk::data::writers::extractors::IntExtractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, IntFieldWriter};
use athena_federation_sdk::data::writers::holders::NullableIntHolder;
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::domain::predicate::ConstraintProjector;

use common_field_writer::{new_vector, CallCounter, MockConstraintProjector};

const EXPECTED: i32 = 123i32;

fn expected() -> i32 {
    EXPECTED
}

struct Fixture {
    vector: FieldVector,
    extractor_calls: CallCounter,
    projector: Arc<MockConstraintProjector>,
}

fn configure_extractor(value: i32, is_set: i32, calls: &CallCounter) -> Arc<dyn IntExtractor> {
    let counter = calls.0.clone();
    Arc::new(move |_ctx: &dyn Any, holder: &mut NullableIntHolder| {
        counter.fetch_add(1, Ordering::SeqCst);
        holder.is_set = is_set;
        holder.value = value;
        Ok(())
    })
}

fn set_up() -> Fixture {
    Fixture {
        vector: new_vector("testVector", DataType::Int32),
        extractor_calls: CallCounter::new(),
        projector: MockConstraintProjector::new(),
    }
}

fn writer(fx: &Fixture, value: i32, is_set: i32, with_constraint: bool) -> IntFieldWriter {
    let constraint: Option<Arc<dyn ConstraintProjector>> = if with_constraint {
        Some(fx.projector.clone())
    } else {
        None
    };
    IntFieldWriter::new(
        configure_extractor(value, is_set, &fx.extractor_calls),
        constraint,
    )
}

fn verify_assertions(fx: &Fixture, expected_result: bool, actual_result: bool) {
    assert_eq!(expected_result, actual_result);
    assert_eq!(fx.vector.get_raw(0), Value::Int(expected()));
}

#[test]
fn write_with_valid_int_value_should_write_successfully() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Int(expected()), true);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, true, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Int(expected())), 1);
}

#[test]
fn write_with_constraint_failure_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Int(expected()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, false, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Int(expected())), 1);
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
fn write_with_null_int_value_should_mark_vector_as_null() {
    let mut fx = set_up();
    let mut w = writer(&fx, 0i32, 0, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
}

#[test]
fn write_with_non_zero_value_marked_null_should_mark_vector_as_null() {
    let mut fx = set_up();
    let mut w = writer(&fx, expected(), 0, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
}

#[test]
fn write_with_constraint_failure_despite_is_set_should_return_false() {
    let mut fx = set_up();
    fx.projector.when_apply(Value::Int(expected()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(!fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(fx.projector.times_called_with(&Value::Int(expected())), 1);
}
