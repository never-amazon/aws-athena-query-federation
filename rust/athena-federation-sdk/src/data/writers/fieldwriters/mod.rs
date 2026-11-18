use std::sync::Arc;

use super::extractors::Extractor;
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub trait FieldWriter<C>: Send {
    fn write(&mut self, vector: &mut FieldVector, context: &C, row_num: usize) -> Result<bool>;
}

pub type FieldWriterFactory<C> = Arc<
    dyn Fn(
            &FieldVector,
            Option<&Extractor<C>>,
            Option<ConstraintProjector>,
        ) -> Result<Box<dyn FieldWriter<C>>>
        + Send
        + Sync,
>;

pub struct ScalarFieldWriter<C> {
    extractor: Extractor<C>,
    constraint: Option<ConstraintProjector>,
}

impl<C> ScalarFieldWriter<C> {
    pub fn new(
        extractor: Extractor<C>,
        constraint: Option<ConstraintProjector>,
    ) -> ScalarFieldWriter<C> {
        ScalarFieldWriter {
            extractor,
            constraint,
        }
    }
}

impl<C> FieldWriter<C> for ScalarFieldWriter<C> {
    fn write(&mut self, vector: &mut FieldVector, context: &C, row_num: usize) -> Result<bool> {
        let extracted = self
            .extractor
            .extract_values(context, vector.field().data_type())?;
        let constraint_value = if let Some((stored, seen)) = extracted {
            vector.set(row_num, &stored)?;
            seen
        } else {
            vector.set_null(row_num);
            Value::Null
        };
        self.constraint
            .as_ref()
            .map_or(Ok(true), |constraint| constraint(&constraint_value))
    }
}
