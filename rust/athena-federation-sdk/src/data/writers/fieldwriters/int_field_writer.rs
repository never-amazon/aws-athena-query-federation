use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::IntExtractor;
use crate::data::writers::holders::NullableIntHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct IntFieldWriter {
    holder: NullableIntHolder,
    extractor: Arc<dyn IntExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl IntFieldWriter {
    pub fn new(
        extractor: Arc<dyn IntExtractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> IntFieldWriter {
        IntFieldWriter {
            holder: NullableIntHolder::default(),
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
                    Value::Int(h.value)
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for IntFieldWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Int(h.value))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
