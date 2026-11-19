mod common_field_writer;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use arrow::datatypes::DataType;

use athena_federation_sdk::data::writers::extractors::Extractor;
use athena_federation_sdk::data::writers::fieldwriters::{FieldWriter, ScalarFieldWriter};
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::domain::predicate::ConstraintProjector;

use common_field_writer::{new_vector, CallCounter, MockConstraintProjector};

fn expected() -> String {
    "Hello, Athena!".to_string()
}

struct Fixture {
    vector: FieldVector,
    extractor_calls: CallCounter,
    projector: Arc<MockConstraintProjector>,
}

fn configure_extractor(value: String, is_set: i32, calls: &CallCounter) -> Extractor<()> {
    let counter = calls.0.clone();
    Extractor::VarChar(Arc::new(move |_ctx: &()| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok((is_set > 0).then(|| value.clone()))
    }))
}

fn set_up() -> Fixture {
    Fixture {
        vector: new_vector("testVarCharVector", DataType::Utf8),
        extractor_calls: CallCounter::new(),
        projector: MockConstraintProjector::new(),
    }
}

fn writer(
    fx: &Fixture,
    value: String,
    is_set: i32,
    with_constraint: bool,
) -> ScalarFieldWriter<()> {
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
    assert_eq!(fx.vector.get_raw(0), Value::String(expected().clone()));
}

#[test]
fn write_with_valid_var_char_value_should_write_successfully() {
    let mut fx = set_up();
    fx.projector
        .when_apply(Value::String(expected().clone()), true);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, true, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector
            .times_called_with(&Value::String(expected().clone())),
        1
    );
}

#[test]
fn write_with_constraint_failure_should_return_false() {
    let mut fx = set_up();
    fx.projector
        .when_apply(Value::String(expected().clone()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    verify_assertions(&fx, false, result);
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector
            .times_called_with(&Value::String(expected().clone())),
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
fn write_with_non_null_value_marked_null_should_mark_vector_as_null() {
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
    fx.projector
        .when_apply(Value::String(expected().clone()), false);
    let mut w = writer(&fx, expected(), 1, true);
    let result = w.write(&mut fx.vector, &(), 0).unwrap();
    assert!(!result);
    assert!(!fx.vector.is_null(0));
    assert_eq!(fx.extractor_calls.count(), 1);
    assert_eq!(
        fx.projector
            .times_called_with(&Value::String(expected().clone())),
        1
    );
}
