use crate::data::{ArrowType, Value};
use crate::exceptions::{FederationError, Result};

use super::marker::{coerce_value, Bound, Marker};

#[derive(Debug, Clone, Copy, Default)]
pub struct MarkerFactory;

impl MarkerFactory {
    pub fn new() -> MarkerFactory {
        MarkerFactory
    }

    pub fn create_nullable(
        &self,
        arrow_type: &ArrowType,
        value: &Value,
        bound: Bound,
    ) -> Result<Marker> {
        if value.is_null() {
            return Ok(Marker::from_parts(
                arrow_type.clone(),
                bound,
                true,
                Value::Null,
            ));
        }
        let stored = coerce_value(arrow_type, value)?;
        Ok(Marker::from_parts(arrow_type.clone(), bound, false, stored))
    }

    pub fn create(&self, arrow_type: &ArrowType, value: &Value, bound: Bound) -> Result<Marker> {
        if value.is_null() {
            return Err(FederationError::null_pointer("value is null"));
        }
        self.create_nullable(arrow_type, value, bound)
    }

    pub fn create_null(&self, arrow_type: &ArrowType, bound: Bound) -> Marker {
        Marker::from_parts(arrow_type.clone(), bound, true, Value::Null)
    }
}
