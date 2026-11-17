use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::java_time::local_date_time_of_epoch_milli_utc;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::DateMilliExtractor;
use crate::data::writers::holders::NullableDateMilliHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct DateMilliFieldWriter {
    holder: NullableDateMilliHolder,
    extractor: Arc<dyn DateMilliExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl DateMilliFieldWriter {
    pub fn new(
        extractor: Arc<dyn DateMilliExtractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> DateMilliFieldWriter {
        DateMilliFieldWriter {
            holder: NullableDateMilliHolder::default(),
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
                    Value::LocalDateTime(local_date_time_of_epoch_milli_utc(h.value))
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for DateMilliFieldWriter {
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
