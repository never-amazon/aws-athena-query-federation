#![allow(
    dead_code,
    reason = "shared test helpers; each test binary uses a subset"
)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Field};
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::domain::predicate::ConstraintProjector;

pub struct MockConstraintProjector {
    stubs: Mutex<Vec<(Value, bool)>>,
    calls: Mutex<Vec<Value>>,
}

impl MockConstraintProjector {
    pub fn new() -> Arc<MockConstraintProjector> {
        Arc::new(MockConstraintProjector {
            stubs: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        })
    }

    pub fn when_apply(&self, value: Value, result: bool) {
        self.stubs.lock().unwrap().push((value, result));
    }

    pub fn times_called_with(&self, value: &Value) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|v| *v == value)
            .count()
    }

    pub fn as_projector(self: &Arc<Self>) -> ConstraintProjector {
        let mock = Arc::clone(self);
        Arc::new(move |value: &Value| Ok(mock.apply(value)))
    }

    fn apply(&self, value: &Value) -> bool {
        self.calls.lock().unwrap().push(value.clone());
        let stubs = self.stubs.lock().unwrap();
        stubs
            .iter()
            .rev()
            .find(|(v, _)| v == value)
            .is_some_and(|(_, r)| *r)
    }
}

pub struct CallCounter(pub Arc<AtomicUsize>);

impl CallCounter {
    pub fn new() -> CallCounter {
        CallCounter(Arc::new(AtomicUsize::new(0)))
    }

    pub fn count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

pub fn new_vector(name: &str, data_type: DataType) -> FieldVector {
    FieldVector::new(Arc::new(Field::new(name, data_type, true)))
}
