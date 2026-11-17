mod big_int_field_writer;
mod bit_field_writer;
mod date_day_field_writer;
mod date_milli_field_writer;
mod decimal_field_writer;
mod float4_field_writer;
mod float8_field_writer;
mod int_field_writer;
mod small_int_field_writer;
mod tiny_int_field_writer;
mod var_binary_field_writer;
mod var_char_field_writer;

pub use big_int_field_writer::BigIntFieldWriter;
pub use bit_field_writer::BitFieldWriter;
pub use date_day_field_writer::DateDayFieldWriter;
pub use date_milli_field_writer::DateMilliFieldWriter;
pub use decimal_field_writer::DecimalFieldWriter;
pub use float4_field_writer::Float4FieldWriter;
pub use float8_field_writer::Float8FieldWriter;
pub use int_field_writer::IntFieldWriter;
pub use small_int_field_writer::SmallIntFieldWriter;
pub use tiny_int_field_writer::TinyIntFieldWriter;
pub use var_binary_field_writer::VarBinaryFieldWriter;
pub use var_char_field_writer::VarCharFieldWriter;

use std::any::Any;
use std::sync::Arc;

use super::extractors::Extractor;
use crate::data::vector::FieldVector;
use crate::domain::predicate::ConstraintProjector;
use crate::exceptions::Result;

pub trait FieldWriter: Send {
    fn write(&mut self, vector: &mut FieldVector, context: &dyn Any, row_num: usize) -> Result<bool>;
}

pub trait FieldWriterFactory: Send + Sync {
    fn create(
        &self,
        vector: &FieldVector,
        extractor: Option<&Extractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> Result<Box<dyn FieldWriter>>;
}

impl<F> FieldWriterFactory for F
where
    F: Fn(&FieldVector, Option<&Extractor>, Option<Arc<dyn ConstraintProjector>>) -> Result<Box<dyn FieldWriter>> + Send + Sync,
{
    fn create(
        &self,
        vector: &FieldVector,
        extractor: Option<&Extractor>,
        constraint: Option<Arc<dyn ConstraintProjector>>,
    ) -> Result<Box<dyn FieldWriter>> {
        self(vector, extractor, constraint)
    }
}
