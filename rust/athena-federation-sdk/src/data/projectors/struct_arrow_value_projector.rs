use super::arrow_value_projector_impl::{create_value_projection, Projection};
use super::ArrowValueProjector;
use crate::data::types::{get_children, minor_type_of};
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub struct StructArrowValueProjector<'a> {
    struct_reader: FieldReader<'a>,
    projections: Vec<(String, Projection)>,
}

impl<'a> StructArrowValueProjector<'a> {
    pub fn new(struct_reader: FieldReader<'a>) -> Result<StructArrowValueProjector<'a>> {
        let projections = get_children(struct_reader.get_field())
            .iter()
            .map(|child| Ok((child.name().clone(), create_value_projection(minor_type_of(child.data_type()))?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(StructArrowValueProjector {
            struct_reader,
            projections,
        })
    }

    pub fn do_project(&self) -> Result<Value> {
        let mut name_to_values = Vec::with_capacity(self.projections.len());
        for (name, projection) in &self.projections {
            let sub_reader = self
                .struct_reader
                .reader_named(name)
                .ok_or_else(|| FederationError::illegal_state(format!("struct reader has no child {}", name)))?;
            name_to_values.push((Value::String(name.clone()), projection(&sub_reader)?));
        }
        Ok(Value::Map(name_to_values))
    }
}

impl ArrowValueProjector for StructArrowValueProjector<'_> {
    fn project(&self, pos: usize) -> Result<Value> {
        self.struct_reader.set_position(pos);
        if !self.struct_reader.is_set() {
            return Ok(Value::Null);
        }
        self.do_project()
    }
}
