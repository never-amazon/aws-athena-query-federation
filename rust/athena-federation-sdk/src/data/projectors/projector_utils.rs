use super::{ArrowValueProjector, ListArrowValueProjector, SimpleArrowValueProjector, StructArrowValueProjector};
use crate::data::types::MinorType;
use crate::data::vector::FieldReader;
use crate::exceptions::Result;

pub struct ProjectorUtils;

impl ProjectorUtils {
    pub fn create_arrow_value_projector<'a>(field_reader: FieldReader<'a>) -> Result<Box<dyn ArrowValueProjector + 'a>> {
        Ok(match field_reader.get_minor_type() {
            MinorType::LIST => Box::new(ListArrowValueProjector::new(field_reader)?),
            MinorType::STRUCT => Box::new(StructArrowValueProjector::new(field_reader)?),
            _ => Box::new(SimpleArrowValueProjector::new(field_reader)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::DataType;

    use super::*;
    use crate::data::block_allocator::{BlockAllocator, BlockAllocatorImpl};
    use crate::data::field_resolver::DEFAULT;
    use crate::data::schema_builder::SchemaBuilder;
    use crate::data::value::Value;

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
            .set_complex_value("l", 0, &DEFAULT, Value::List(vec![Value::Int(1), Value::Null, Value::Int(3)]))
            .unwrap();
        block
            .set_complex_value("s", 0, &DEFAULT, Value::struct_of([("a", Value::from("x"))]))
            .unwrap();
        block.set_row_count(2);

        let d = ProjectorUtils::create_arrow_value_projector(block.get_field_reader("d").unwrap()).unwrap();
        assert_eq!(
            d.project(0).unwrap(),
            Value::LocalDate(chrono::NaiveDate::from_ymd_opt(1970, 1, 11).unwrap())
        );
        assert_eq!(d.project(1).unwrap(), Value::Null);

        let l = ProjectorUtils::create_arrow_value_projector(block.get_field_reader("l").unwrap()).unwrap();
        assert_eq!(l.project(0).unwrap(), Value::List(vec![Value::Int(1), Value::Null, Value::Int(3)]));
        assert_eq!(l.project(1).unwrap(), Value::Null);

        let s = ProjectorUtils::create_arrow_value_projector(block.get_field_reader("s").unwrap()).unwrap();
        assert_eq!(s.project(0).unwrap(), Value::struct_of([("a", Value::from("x"))]));
        assert_eq!(s.project(1).unwrap(), Value::Null);
    }
}
