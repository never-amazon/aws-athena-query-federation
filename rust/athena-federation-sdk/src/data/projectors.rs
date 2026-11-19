use crate::data::java_time::local_date_of_epoch_day;
use crate::data::types::{get_children, minor_type_of, MinorType};
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub enum ArrowValueProjector<'a> {
    Simple(SimpleArrowValueProjector<'a>),
    List(ListArrowValueProjector<'a>),
    Struct(StructArrowValueProjector<'a>),
}

impl<'a> ArrowValueProjector<'a> {
    pub fn new(field_reader: FieldReader<'a>) -> Result<ArrowValueProjector<'a>> {
        Ok(match field_reader.minor_type() {
            MinorType::List => {
                ArrowValueProjector::List(ListArrowValueProjector::new(field_reader)?)
            }
            MinorType::Struct => {
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

pub type Projection = fn(&FieldReader<'_>) -> Result<Value>;

fn opt<T: Into<Value>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, Into::into)
}

pub fn create_value_projection(minor_type: MinorType) -> Result<Projection> {
    Ok(match minor_type {
        MinorType::List => |r| ListArrowValueProjector::new(*r)?.do_project(),
        MinorType::Struct => |r| StructArrowValueProjector::new(*r)?.do_project(),
        MinorType::DateMilli => |r| Ok(opt(r.read_local_date_time())),
        MinorType::TinyInt | MinorType::UInt1 => |r| Ok(opt(r.read_byte())),
        MinorType::UInt2 => |r| Ok(r.read_character().map_or(Value::Null, Value::Char)),
        MinorType::SmallInt => |r| Ok(opt(r.read_short())),
        MinorType::DateDay => |r| match r.read_integer() {
            None => Ok(Value::Null),
            Some(days) => local_date_of_epoch_day(i64::from(days))
                .map(Value::LocalDate)
                .ok_or_else(|| FederationError::Arithmetic(format!("Invalid epoch day {days}"))),
        },
        MinorType::Int | MinorType::UInt4 => |r| Ok(opt(r.read_integer())),
        MinorType::UInt8 | MinorType::BigInt => |r| Ok(opt(r.read_long())),
        MinorType::Decimal => |r| Ok(opt(r.read_big_decimal())),
        MinorType::Float4 => |r| Ok(opt(r.read_float())),
        MinorType::Float8 => |r| Ok(opt(r.read_double())),
        MinorType::VarChar => |r| Ok(opt(r.read_text())),
        MinorType::VarBinary => |r| Ok(opt(r.read_byte_array())),
        MinorType::Bit => |r| Ok(opt(r.read_boolean())),
        other => {
            return Err(FederationError::invalid_input(format!(
                "Unsupported type {other}"
            )))
        }
    })
}

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

    pub fn project(&self, pos: usize) -> Result<Value> {
        (self.projection)(&self.field_reader.at(pos))
    }
}

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
        self.project_at(self.list_reader)
    }

    fn project_at(&self, list_reader: FieldReader<'a>) -> Result<Value> {
        let mut list = Vec::new();
        for sub_reader in list_reader.elements() {
            if !sub_reader.is_set() {
                list.push(Value::Null);
                continue;
            }
            list.push((self.projection)(&sub_reader)?);
        }
        Ok(Value::List(list))
    }

    pub fn project(&self, pos: usize) -> Result<Value> {
        let list_reader = self.list_reader.at(pos);
        if !list_reader.is_set() {
            return Ok(Value::Null);
        }
        self.project_at(list_reader)
    }
}

pub struct StructArrowValueProjector<'a> {
    struct_reader: FieldReader<'a>,
    projections: Vec<(String, Projection)>,
}

impl<'a> StructArrowValueProjector<'a> {
    pub fn new(struct_reader: FieldReader<'a>) -> Result<StructArrowValueProjector<'a>> {
        let projections = get_children(struct_reader.field())
            .iter()
            .map(|child| {
                Ok((
                    child.name().clone(),
                    create_value_projection(minor_type_of(child.data_type()))?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(StructArrowValueProjector {
            struct_reader,
            projections,
        })
    }

    pub fn do_project(&self) -> Result<Value> {
        self.project_at(self.struct_reader)
    }

    fn project_at(&self, struct_reader: FieldReader<'a>) -> Result<Value> {
        let mut name_to_values = Vec::with_capacity(self.projections.len());
        for (name, projection) in &self.projections {
            let sub_reader = struct_reader.reader_named(name).ok_or_else(|| {
                FederationError::illegal_state(format!("struct reader has no child {name}"))
            })?;
            name_to_values.push((Value::String(name.clone()), projection(&sub_reader)?));
        }
        Ok(Value::Map(name_to_values))
    }

    pub fn project(&self, pos: usize) -> Result<Value> {
        let struct_reader = self.struct_reader.at(pos);
        if !struct_reader.is_set() {
            return Ok(Value::Null);
        }
        self.project_at(struct_reader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::block_allocator::{BlockAllocator, BlockAllocatorImpl};
    use crate::data::schema_builder::SchemaBuilder;
    use crate::data::value::Value;
    use crate::data::DefaultFieldResolver;
    use arrow::datatypes::DataType;
    use std::sync::Arc;

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
