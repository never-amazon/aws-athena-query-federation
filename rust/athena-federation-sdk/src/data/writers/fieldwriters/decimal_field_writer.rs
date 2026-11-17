use std::any::Any;
use std::sync::Arc;

use bigdecimal::RoundingMode;

use super::FieldWriter;
use crate::data::types::decimal_precision_scale;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::DecimalExtractor;
use crate::data::writers::holders::NullableDecimalHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct DecimalFieldWriter {
    holder: NullableDecimalHolder,
    extractor: Arc<dyn DecimalExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl DecimalFieldWriter {
    pub fn new(
        extractor: Arc<dyn DecimalExtractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> DecimalFieldWriter {
        DecimalFieldWriter {
            holder: NullableDecimalHolder::default(),
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
                    Value::Decimal(h.value.clone())
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for DecimalFieldWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            let (_, scale) =
                decimal_precision_scale(vector.get_field().data_type()).unwrap_or((38, 0));
            let scaled = h
                .value
                .with_scale_round(i64::from(scale), RoundingMode::HalfUp);
            vector.set(row_num, &Value::Decimal(scaled))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
