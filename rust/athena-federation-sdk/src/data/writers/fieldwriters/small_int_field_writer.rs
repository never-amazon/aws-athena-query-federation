use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::vector::FieldVector;
use crate::data::value::Value;
use crate::data::writers::extractors::SmallIntExtractor;
use crate::data::writers::holders::NullableSmallIntHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct SmallIntFieldWriter {
    holder: NullableSmallIntHolder,
    extractor: Arc<dyn SmallIntExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl SmallIntFieldWriter {
    pub fn new(extractor: Arc<dyn SmallIntExtractor>, constraint: Option<Arc<dyn ConstraintProjector>>) -> SmallIntFieldWriter {
        SmallIntFieldWriter {
            holder: NullableSmallIntHolder::default(),
            extractor,
            constraint,
        }
    }

    fn apply_constraint(&self) -> Result<bool> {
        let h = &self.holder;
        match &self.constraint {
            Some(c) => {
                let value = if h.is_set == 0 { Value::Null } else { Value::Short(h.value) };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for SmallIntFieldWriter {
    fn write(&mut self, vector: &mut FieldVector, context: &dyn Any, row_num: usize) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Short(h.value))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
