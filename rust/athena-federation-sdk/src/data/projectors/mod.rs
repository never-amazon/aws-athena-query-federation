mod arrow_value_projector_impl;
mod list_arrow_value_projector;
mod simple_arrow_value_projector;
mod struct_arrow_value_projector;

pub use arrow_value_projector_impl::{create_value_projection, Projection};
pub use list_arrow_value_projector::ListArrowValueProjector;
pub use simple_arrow_value_projector::SimpleArrowValueProjector;
pub use struct_arrow_value_projector::StructArrowValueProjector;

use crate::data::types::MinorType;
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::Result;

pub enum ArrowValueProjector<'a> {
    Simple(SimpleArrowValueProjector<'a>),
    List(ListArrowValueProjector<'a>),
    Struct(StructArrowValueProjector<'a>),
}

impl<'a> ArrowValueProjector<'a> {
    pub fn new(field_reader: FieldReader<'a>) -> Result<ArrowValueProjector<'a>> {
        Ok(match field_reader.minor_type() {
            MinorType::LIST => {
                ArrowValueProjector::List(ListArrowValueProjector::new(field_reader)?)
            }
            MinorType::STRUCT => {
                ArrowValueProjector::Struct(StructArrowValueProjector::new(field_reader)?)
            }
            _ => ArrowValueProjector::Simple(SimpleArrowValueProjector::new(field_reader)?),
        })
    }

    pub fn project(&self, pos: usize) -> Result<Value> {
        match self {
            ArrowValueProjector::Simple(p) => p.project(pos),
            ArrowValueProjector::List(p) => p.project(pos),
            ArrowValueProjector::Struct(p) => p.project(pos),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::DataType;

    use super::*;
    use crate::data::block_allocator::{BlockAllocator, BlockAllocatorImpl};
    use crate::data::schema_builder::SchemaBuilder;
    use crate::data::value::Value;
    use crate::data::DefaultFieldResolver;

    #[test]
    fn projects_simple_list_and_struct_values() {
        let schema = SchemaBuilder::new()
            .add_date_day_field("d")
            .add_list_field("l", DataType::Int32)
            .add_struct_field("s")
            .add_child_field("s", "a", DataType::Utf8)
            .build();
        let allocator = BlockAllocatorImpl::new();
        let mut block = allocator.create_block(Arc::new(schema));
        block.set_value("d", 0, 10i32).unwrap();
        block
            .set_complex_value(
                "l",
                0,
                &DefaultFieldResolver,
                Value::List(vec![Value::Int(1), Value::Null, Value::Int(3)]),
            )
            .unwrap();
        block
            .set_complex_value(
                "s",
                0,
                &DefaultFieldResolver,
                Value::struct_of([("a", Value::from("x"))]),
            )
            .unwrap();
        block.set_row_count(2);

        let d = ArrowValueProjector::new(block.get_field_reader("d").unwrap()).unwrap();
        assert_eq!(
            d.project(0).unwrap(),
            Value::LocalDate(chrono::NaiveDate::from_ymd_opt(1970, 1, 11).unwrap())
        );
        assert_eq!(d.project(1).unwrap(), Value::Null);

        let l = ArrowValueProjector::new(block.get_field_reader("l").unwrap()).unwrap();
        assert_eq!(
            l.project(0).unwrap(),
            Value::List(vec![Value::Int(1), Value::Null, Value::Int(3)])
        );
        assert_eq!(l.project(1).unwrap(), Value::Null);

        let s = ArrowValueProjector::new(block.get_field_reader("s").unwrap()).unwrap();
        assert_eq!(
            s.project(0).unwrap(),
            Value::struct_of([("a", Value::from("x"))])
        );
        assert_eq!(s.project(1).unwrap(), Value::Null);
    }
}
