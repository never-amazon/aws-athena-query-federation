use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::vector::FieldVector;
use crate::data::value::Value;
use crate::data::writers::extractors::BitExtractor;
use crate::data::writers::holders::NullableBitHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct BitFieldWriter {
    holder: NullableBitHolder,
    extractor: Arc<dyn BitExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl BitFieldWriter {
    pub fn new(extractor: Arc<dyn BitExtractor>, constraint: Option<Arc<dyn ConstraintProjector>>) -> BitFieldWriter {
        BitFieldWriter {
            holder: NullableBitHolder::default(),
            extractor,
            constraint,
        }
    }

    fn apply_constraint(&self) -> Result<bool> {
        let h = &self.holder;
        match &self.constraint {
            Some(c) => {
                let value = if h.is_set == 0 { Value::Null } else { Value::Boolean(h.value > 0) };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for BitFieldWriter {
    fn write(&mut self, vector: &mut FieldVector, context: &dyn Any, row_num: usize) -> Result<bool> {
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
