use super::arrow_value_projector_impl::{create_value_projection, Projection};
use super::ArrowValueProjector;
use crate::data::types::{get_children, minor_type_of};
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub struct ListArrowValueProjector<'a> {
    list_reader: FieldReader<'a>,
    projection: Projection,
}

impl<'a> ListArrowValueProjector<'a> {
    pub fn new(list_reader: FieldReader<'a>) -> Result<ListArrowValueProjector<'a>> {
        let children = get_children(list_reader.field());
        if children.len() != 1 {
            return Err(FederationError::invalid_input(format!(
                "Unexpected number of children for ListProjector field {}",
                list_reader.field().name()
            )));
        }
        let projection = create_value_projection(minor_type_of(children[0].data_type()))?;
        Ok(ListArrowValueProjector {
            list_reader,
            projection,
        })
    }

    pub fn do_project(&self) -> Result<Value> {
        let mut list = Vec::new();
        while self.list_reader.next() {
            let sub_reader = self
                .list_reader
                .reader()
                .ok_or_else(|| FederationError::illegal_state("list reader has no child reader"))?;
            if !sub_reader.is_set() {
                list.push(Value::Null);
                continue;
            }
            list.push((self.projection)(&sub_reader)?);
        }
        Ok(Value::List(list))
    }
}

impl ArrowValueProjector for ListArrowValueProjector<'_> {
    fn project(&self, pos: usize) -> Result<Value> {
        self.list_reader.set_position(pos);
        if !self.list_reader.is_set() {
            return Ok(Value::Null);
        }
        self.do_project()
    }
}
