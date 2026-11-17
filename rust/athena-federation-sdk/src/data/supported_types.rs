use arrow::datatypes::Field;

use super::types::{arrow_type_to_string, get_children, minor_type_of, ArrowType, MinorType};
use crate::exceptions::{FederationError, Result};

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedTypes {
    BIT,
    DATEMILLI,
    DATEDAY,
    TIMESTAMPMILLITZ,
    TIMESTAMPMICROTZ,
    FLOAT8,
    FLOAT4,
    INT,
    TINYINT,
    SMALLINT,
    BIGINT,
    VARBINARY,
    DECIMAL,
    VARCHAR,
    STRUCT,
    LIST,
    MAP,
}

impl SupportedTypes {
    pub fn values() -> &'static [SupportedTypes] {
        use SupportedTypes::*;
        &[
            BIT, DATEMILLI, DATEDAY, TIMESTAMPMILLITZ, TIMESTAMPMICROTZ, FLOAT8, FLOAT4, INT, TINYINT, SMALLINT, BIGINT,
            VARBINARY, DECIMAL, VARCHAR, STRUCT, LIST, MAP,
        ]
    }

    pub fn get_arrow_minor_type(&self) -> MinorType {
        match self {
            SupportedTypes::BIT => MinorType::BIT,
            SupportedTypes::DATEMILLI => MinorType::DATEMILLI,
            SupportedTypes::DATEDAY => MinorType::DATEDAY,
            SupportedTypes::TIMESTAMPMILLITZ => MinorType::TIMESTAMPMILLITZ,
            SupportedTypes::TIMESTAMPMICROTZ => MinorType::TIMESTAMPMICROTZ,
            SupportedTypes::FLOAT8 => MinorType::FLOAT8,
            SupportedTypes::FLOAT4 => MinorType::FLOAT4,
            SupportedTypes::INT => MinorType::INT,
            SupportedTypes::TINYINT => MinorType::TINYINT,
            SupportedTypes::SMALLINT => MinorType::SMALLINT,
            SupportedTypes::BIGINT => MinorType::BIGINT,
            SupportedTypes::VARBINARY => MinorType::VARBINARY,
            SupportedTypes::DECIMAL => MinorType::DECIMAL,
            SupportedTypes::VARCHAR => MinorType::VARCHAR,
            SupportedTypes::STRUCT => MinorType::STRUCT,
            SupportedTypes::LIST => MinorType::LIST,
            SupportedTypes::MAP => MinorType::MAP,
        }
    }

    pub fn is_supported_minor_type(minor_type: MinorType) -> bool {
        SupportedTypes::values()
            .iter()
            .any(|t| t.get_arrow_minor_type() == minor_type)
    }

    pub fn is_supported_arrow_type(arrow_type: &ArrowType) -> bool {
        SupportedTypes::is_supported_minor_type(minor_type_of(arrow_type))
    }

    pub fn is_supported(field: &Field) -> bool {
        if !SupportedTypes::is_supported_minor_type(minor_type_of(field.data_type())) {
            return false;
        }
        get_children(field).iter().all(|c| SupportedTypes::is_supported(c))
    }

    pub fn assert_supported(field: &Field) -> Result<()> {
        if !SupportedTypes::is_supported(field) {
            return Err(FederationError::not_supported(format!(
                "Detected unsupported type[{} / {} for column[{}]",
                arrow_type_to_string(field.data_type()),
                minor_type_of(field.data_type()),
                field.name()
            )));
        }
        Ok(())
    }
}
