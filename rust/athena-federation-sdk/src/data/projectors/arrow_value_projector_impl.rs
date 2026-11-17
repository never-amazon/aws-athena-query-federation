use super::list_arrow_value_projector::ListArrowValueProjector;
use super::struct_arrow_value_projector::StructArrowValueProjector;
use crate::data::java_time::local_date_of_epoch_day;
use crate::data::types::MinorType;
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub type Projection = fn(&FieldReader<'_>) -> Result<Value>;

fn opt<T: Into<Value>>(v: Option<T>) -> Result<Value> {
    Ok(v.map(Into::into).unwrap_or(Value::Null))
}

pub fn create_value_projection(minor_type: MinorType) -> Result<Projection> {
    Ok(match minor_type {
        MinorType::LIST => |r| ListArrowValueProjector::new(r.clone())?.do_project(),
        MinorType::STRUCT => |r| StructArrowValueProjector::new(r.clone())?.do_project(),
        MinorType::DATEMILLI => |r| opt(r.read_local_date_time()),
        MinorType::TINYINT | MinorType::UINT1 => |r| opt(r.read_byte()),
        MinorType::UINT2 => |r| Ok(r.read_character().map(Value::Char).unwrap_or(Value::Null)),
        MinorType::SMALLINT => |r| opt(r.read_short()),
        MinorType::DATEDAY => |r| match r.read_integer() {
            None => Ok(Value::Null),
            Some(days) => local_date_of_epoch_day(days as i64)
                .map(Value::LocalDate)
                .ok_or_else(|| FederationError::Arithmetic(format!("Invalid epoch day {}", days))),
        },
        MinorType::INT | MinorType::UINT4 => |r| opt(r.read_integer()),
        MinorType::UINT8 | MinorType::BIGINT => |r| opt(r.read_long()),
        MinorType::DECIMAL => |r| opt(r.read_big_decimal()),
        MinorType::FLOAT4 => |r| opt(r.read_float()),
        MinorType::FLOAT8 => |r| opt(r.read_double()),
        MinorType::VARCHAR => |r| opt(r.read_text()),
        MinorType::VARBINARY => |r| opt(r.read_byte_array()),
        MinorType::BIT => |r| opt(r.read_boolean()),
        other => return Err(FederationError::invalid_input(format!("Unsupported type {}", other))),
    })
}
