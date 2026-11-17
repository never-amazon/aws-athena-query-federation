use std::collections::HashMap;

use arrow::datatypes::DataType;
use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::{FederationError, Result};
use athena_jdbc::manager::get_arrow_type_from_jdbc_type;
use athena_jdbc::sql::Types;

const DEFAULT_SCALE_KEY: &str = "default_scale";

pub struct SnowflakeArrowTypeConverter;

impl SnowflakeArrowTypeConverter {
    pub const DEFAULT_PRECISION: i32 = 38;

    pub fn to_arrow_type(
        jdbc_type: i32,
        precision: i32,
        scale: i32,
        config_options: &HashMap<String, String>,
    ) -> Result<Option<ArrowType>> {
        let default_scale_text = config_options
            .get(DEFAULT_SCALE_KEY)
            .map_or("0", String::as_str);
        let default_scale = default_scale_text.parse::<i32>().map_err(|_| {
            FederationError::NumberFormat(format!("For input string: \"{default_scale_text}\""))
        })?;
        let mut resolved_precision = precision;
        let mut resolved_scale = scale;
        let needs_resolving = jdbc_type == Types::NUMERIC && precision == 0 && scale <= 0;
        let decimal_exceeding_precision =
            jdbc_type == Types::DECIMAL && precision > Self::DEFAULT_PRECISION;
        if needs_resolving {
            resolved_precision = Self::DEFAULT_PRECISION;
            resolved_scale = default_scale;
        } else if decimal_exceeding_precision {
            resolved_precision = Self::DEFAULT_PRECISION;
        }
        if jdbc_type == Types::BIGINT {
            return Ok(Some(decimal(resolved_precision, resolved_scale)));
        }
        match get_arrow_type_from_jdbc_type(jdbc_type, resolved_precision, resolved_scale, None) {
            Ok(DataType::Date32 | DataType::Date64) => Ok(Some(DataType::Date32)),
            Ok(DataType::Timestamp(_, _)) => Ok(Some(DataType::Date64)),
            Ok(arrow_type) => Ok(Some(arrow_type)),
            Err(FederationError::UnsupportedOperation(message)) => {
                tracing::warn!("Error converting JDBC Type [{jdbc_type}] to arrow: {message}");
                if jdbc_type == Types::TIMESTAMP_WITH_TIMEZONE {
                    tracing::debug!("Converting JDBC Type [{jdbc_type}] to arrow: {message}");
                    return Ok(Some(DataType::Date64));
                }
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }
}

fn decimal(precision: i32, scale: i32) -> ArrowType {
    let precision = u8::try_from(precision).unwrap_or(if precision < 0 { 0 } else { u8::MAX });
    let scale = i8::try_from(scale).unwrap_or(if scale < 0 { i8::MIN } else { i8::MAX });
    DataType::Decimal128(precision, scale)
}
