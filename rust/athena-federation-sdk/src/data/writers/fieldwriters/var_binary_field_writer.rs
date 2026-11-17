use std::any::Any;
use std::sync::Arc;

use super::FieldWriter;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::data::writers::extractors::VarBinaryExtractor;
use crate::data::writers::holders::NullableVarBinaryHolder;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub struct VarBinaryFieldWriter {
    holder: NullableVarBinaryHolder,
    extractor: Arc<dyn VarBinaryExtractor>,
    constraint: Option<Arc<dyn ConstraintProjector>>,
}

impl VarBinaryFieldWriter {
    pub fn new(
        extractor: Arc<dyn VarBinaryExtractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> VarBinaryFieldWriter {
        VarBinaryFieldWriter {
            holder: NullableVarBinaryHolder::default(),
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
                    Value::Bytes(h.value.clone())
                };
                c.apply(&value)
            }
            None => Ok(true),
        }
    }
}

impl FieldWriter for VarBinaryFieldWriter {
    fn write(
        &mut self,
        vector: &mut FieldVector,
        context: &dyn Any,
        row_num: usize,
    ) -> Result<bool> {
        self.extractor.extract(context, &mut self.holder)?;
        let h = &self.holder;
        if h.is_set > 0 {
            vector.set(row_num, &Value::Bytes(h.value.clone()))?;
        } else {
            vector.set_null(row_num);
        }
        self.apply_constraint()
    }
}
