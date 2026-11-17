use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::vector::FieldVector;
use crate::data::value::Value;
use crate::data::writers::extractors::DateDayExtractor;
use crate::data::writers::holders::NullableDateDayHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct DateDayFieldWriter {
    holder: NullableDateDayHolder,
    extractor: Arc<dyn DateDayExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl DateDayFieldWriter {
    pub fn new(extractor: Arc<dyn DateDayExtractor>, constraint: Option<Arc<dyn ConstraintProjector>>) -> DateDayFieldWriter {
        DateDayFieldWriter {
            holder: NullableDateDayHolder::default(),
            extractor,
            constraint,
        }
    }

    fn apply_constraint(&self) -> Result<bool> {
        let h = &self.holder;
        match &self.constraint {
            Some(c) => {
                let value = if h.is_set == 0 { Value::Null } else { Value::Int(h.value) };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for DateDayFieldWriter {
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
