use arrow::datatypes::Field;

use super::types::{arrow_type_to_string, get_children, minor_type_of, MinorType};
use crate::exceptions::{FederationError, Result};

impl MinorType {
    pub fn is_supported(self) -> bool {
        matches!(
            self,
            MinorType::Bit
                | MinorType::DateMilli
                | MinorType::DateDay
                | MinorType::TimestampMilliTz
                | MinorType::TimestampMicroTz
                | MinorType::Float8
                | MinorType::Float4
                | MinorType::Int
                | MinorType::TinyInt
                | MinorType::SmallInt
                | MinorType::BigInt
                | MinorType::VarBinary
                | MinorType::Decimal
                | MinorType::VarChar
                | MinorType::Struct
                | MinorType::List
                | MinorType::Map
        )
    }
}

fn is_supported_field(field: &Field) -> bool {
    minor_type_of(field.data_type()).is_supported()
        && get_children(field).iter().all(|c| is_supported_field(c))
}

pub fn assert_supported(field: &Field) -> Result<()> {
    if !is_supported_field(field) {
        return Err(FederationError::not_supported(format!(
            "Detected unsupported type[{} / {} for column[{}]",
            arrow_type_to_string(field.data_type()),
            minor_type_of(field.data_type()),
            field.name()
        )));
    }
    Ok(())
}
