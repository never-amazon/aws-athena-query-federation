use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::BigIntExtractor;
use crate::data::writers::holders::NullableBigIntHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct BigIntFieldWriter {
    holder: NullableBigIntHolder,
    extractor: Arc<dyn BigIntExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl BigIntFieldWriter {
    pub fn new(
        extractor: Arc<dyn BigIntExtractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> BigIntFieldWriter {
        BigIntFieldWriter {
            holder: NullableBigIntHolder::default(),
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
                    Value::Long(h.value)
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for BigIntFieldWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Long(h.value))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
