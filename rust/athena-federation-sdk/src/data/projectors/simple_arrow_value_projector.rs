use super::arrow_value_projector_impl::{create_value_projection, Projection};
use super::ArrowValueProjector;
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::Result;

pub struct SimpleArrowValueProjector<'a> {
    field_reader: FieldReader<'a>,
    projection: Projection,
}

impl<'a> SimpleArrowValueProjector<'a> {
    pub fn new(field_reader: FieldReader<'a>) -> Result<SimpleArrowValueProjector<'a>> {
        let projection = create_value_projection(field_reader.minor_type())?;
        Ok(SimpleArrowValueProjector {
            field_reader,
            projection,
        })
    }
}

impl ArrowValueProjector for SimpleArrowValueProjector<'_> {
    fn project(&self, pos: usize) -> Result<Value> {
        self.field_reader.set_position(pos);
        (self.projection)(&self.field_reader)
    }
}
