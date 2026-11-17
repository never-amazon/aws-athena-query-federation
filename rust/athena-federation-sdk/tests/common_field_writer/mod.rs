#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use arrow::datatypes::{DataType, Field};
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::domain::predicate::ConstraintProjector;
use athena_federation_sdk::Result;

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
        self.calls.lock().unwrap().iter().filter(|v| *v == value).count()
    }
}

impl ConstraintProjector for MockConstraintProjector {
    fn apply(&self, value: &Value) -> Result<bool> {
        self.calls.lock().unwrap().push(value.clone());
        let stubs = self.stubs.lock().unwrap();
        Ok(stubs
            .iter()
            .rev()
            .find(|(v, _)| v == value)
            .map(|(_, r)| *r)
            .unwrap_or(false))
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
