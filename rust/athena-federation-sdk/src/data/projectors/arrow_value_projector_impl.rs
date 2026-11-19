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
