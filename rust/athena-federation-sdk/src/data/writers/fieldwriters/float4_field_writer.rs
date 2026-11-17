use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::vector::FieldVector;
use crate::data::value::Value;
use crate::data::writers::extractors::Float4Extractor;
use crate::data::writers::holders::NullableFloat4Holder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct Float4FieldWriter {
    holder: NullableFloat4Holder,
    extractor: Arc<dyn Float4Extractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl Float4FieldWriter {
    pub fn new(extractor: Arc<dyn Float4Extractor>, constraint: Option<Arc<dyn ConstraintProjector>>) -> Float4FieldWriter {
        Float4FieldWriter {
            holder: NullableFloat4Holder::default(),
            extractor,
            constraint,
        }
    }

    fn apply_constraint(&self) -> Result<bool> {
        let h = &self.holder;
        match &self.constraint {
            Some(c) => {
                let value = if h.is_set == 0 { Value::Null } else { Value::Float(h.value) };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for Float4FieldWriter {
    fn write(&mut self, vector: &mut FieldVector, context: &dyn Any, row_num: usize) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Float(h.value))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
