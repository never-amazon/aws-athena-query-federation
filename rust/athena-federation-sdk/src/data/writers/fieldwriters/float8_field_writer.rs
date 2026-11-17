use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::Float8Extractor;
use crate::data::writers::holders::NullableFloat8Holder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct Float8FieldWriter {
    holder: NullableFloat8Holder,
    extractor: Arc<dyn Float8Extractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl Float8FieldWriter {
    pub fn new(
        extractor: Arc<dyn Float8Extractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> Float8FieldWriter {
        Float8FieldWriter {
            holder: NullableFloat8Holder::default(),
            extractor,
            constraint,
        }
    }

    fn apply_constraint(&self) -> Result<bool> {
        let h = &self.holder;
        match &self.constraint {
            Some(c) => {
                let value = if h.is_set == 0 {
                    Value::Null
                } else {
                    Value::Double(h.value)
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for Float8FieldWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Double(h.value))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
