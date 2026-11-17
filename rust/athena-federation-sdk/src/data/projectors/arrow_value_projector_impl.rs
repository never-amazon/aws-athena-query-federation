use super::list_arrow_value_projector::ListArrowValueProjector;
use super::struct_arrow_value_projector::StructArrowValueProjector;
use crate::data::java_time::local_date_of_epoch_day;
use crate::data::types::MinorType;
use crate::data::value::Value;
use crate::data::vector::FieldReader;
use crate::exceptions::{FederationError, Result};

pub type Projection = fn(&FieldReader<'_>) -> Result<Value>;

fn opt<T: Into<Value>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, Into::into)
}

pub fn create_value_projection(minor_type: MinorType) -> Result<Projection> {
    Ok(match minor_type {
        MinorType::LIST => |r| ListArrowValueProjector::new(r.clone())?.do_project(),
        MinorType::STRUCT => |r| StructArrowValueProjector::new(r.clone())?.do_project(),
        MinorType::DATEMILLI => |r| Ok(opt(r.read_local_date_time())),
        MinorType::TINYINT | MinorType::UINT1 => |r| Ok(opt(r.read_byte())),
        MinorType::UINT2 => |r| Ok(r.read_character().map_or(Value::Null, Value::Char)),
        MinorType::SMALLINT => |r| Ok(opt(r.read_short())),
        MinorType::DATEDAY => |r| match r.read_integer() {
            None => Ok(Value::Null),
            Some(days) => local_date_of_epoch_day(i64::from(days))
                .map(Value::LocalDate)
                .ok_or_else(|| FederationError::Arithmetic(format!("Invalid epoch day {days}"))),
        },
        MinorType::INT | MinorType::UINT4 => |r| Ok(opt(r.read_integer())),
        MinorType::UINT8 | MinorType::BIGINT => |r| Ok(opt(r.read_long())),
        MinorType::DECIMAL => |r| Ok(opt(r.read_big_decimal())),
        MinorType::FLOAT4 => |r| Ok(opt(r.read_float())),
        MinorType::FLOAT8 => |r| Ok(opt(r.read_double())),
        MinorType::VARCHAR => |r| Ok(opt(r.read_text())),
        MinorType::VARBINARY => |r| Ok(opt(r.read_byte_array())),
        MinorType::BIT => |r| Ok(opt(r.read_boolean())),
        other => {
            return Err(FederationError::invalid_input(format!(
                "Unsupported type {other}"
            )))
        }
    })
}
