use std::collections::HashMap;
use std::hash::BuildHasher;

use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};
use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::{FederationError, Result};

use crate::sql::{JdbcType, Nullability, ResultSetMetaData};

const MAX_DECIMAL128_PRECISION: i32 = 38;

pub fn get_arrow_type_from_jdbc_type(
    jdbc_type: JdbcType,
    precision: i32,
    scale: i32,
    time_zone: Option<&str>,
) -> Result<ArrowType> {
    let arrow_type = match jdbc_type {
        JdbcType::Boolean | JdbcType::Bit => DataType::Boolean,
        JdbcType::TinyInt => DataType::Int8,
        JdbcType::SmallInt => DataType::Int16,
        JdbcType::Integer => DataType::Int32,
        JdbcType::BigInt => DataType::Int64,
        JdbcType::Numeric | JdbcType::Decimal => decimal_type(precision, scale),
        JdbcType::Real | JdbcType::Float => DataType::Float32,
        JdbcType::Double => DataType::Float64,
        JdbcType::Char
        | JdbcType::NChar
        | JdbcType::Varchar
        | JdbcType::NVarchar
        | JdbcType::LongVarchar
        | JdbcType::LongNVarchar
        | JdbcType::Clob
        | JdbcType::NClob => DataType::Utf8,
        JdbcType::Date => DataType::Date32,
        JdbcType::Time => DataType::Time32(TimeUnit::Millisecond),
        JdbcType::Timestamp => {
            DataType::Timestamp(TimeUnit::Millisecond, time_zone.map(Into::into))
        }
        JdbcType::Binary | JdbcType::VarBinary | JdbcType::LongVarBinary | JdbcType::Blob => {
            DataType::Binary
        }
        JdbcType::Array => DataType::List(Field::new("$data$", DataType::Null, true).into()),
        JdbcType::Null => DataType::Null,
        JdbcType::Struct => DataType::Struct(Fields::empty()),
        other => {
            return Err(FederationError::UnsupportedOperation(format!(
                "Unmapped JDBC type: {}",
                other.code()
            )))
        }
    };
    Ok(arrow_type)
}

fn decimal_type(precision: i32, scale: i32) -> ArrowType {
    let scale = i8::try_from(scale).unwrap_or(if scale < 0 { i8::MIN } else { i8::MAX });
    let narrow_precision =
        u8::try_from(precision).unwrap_or(if precision < 0 { 0 } else { u8::MAX });
    if precision > MAX_DECIMAL128_PRECISION {
        DataType::Decimal256(narrow_precision, scale)
    } else {
        DataType::Decimal128(narrow_precision, scale)
    }
}

pub fn jdbc_to_arrow_schema(metadata: &ResultSetMetaData) -> Result<Schema> {
    let mut fields = Vec::with_capacity(metadata.column_count());
    for (index, column) in metadata.columns().iter().enumerate() {
        let arrow_type = get_arrow_type_from_jdbc_type(
            column.jdbc_type(),
            column.precision(),
            column.scale(),
            Some("UTC"),
        )?;
        if matches!(arrow_type, DataType::List(_)) {
            return Err(FederationError::IllegalArgument(format!(
                "Configuration does not provide a mapping for array column {}",
                index + 1
            )));
        }
        let nullable = column.nullability() != Nullability::NoNulls;
        fields.push(Field::new(column.label(), arrow_type, nullable));
    }
    Ok(Schema::new(fields))
}

pub const DEFAULT_PRECISION: i32 = 38;
const DEFAULT_SCALE_KEY: &str = "default_scale";

pub fn jdbc_to_arrow_type<S: BuildHasher>(
    jdbc_type: JdbcType,
    precision: i32,
    scale: i32,
    config_options: &HashMap<String, String, S>,
) -> Result<Option<ArrowType>> {
    let default_scale_text = config_options
        .get(DEFAULT_SCALE_KEY)
        .map_or("0", String::as_str);
    let default_scale = default_scale_text.parse::<i32>().map_err(|_| {
        FederationError::NumberFormat(format!("For input string: \"{default_scale_text}\""))
    })?;
    let mut resolved_precision = precision;
    let mut resolved_scale = scale;
    let needs_resolving = jdbc_type == JdbcType::Numeric && precision == 0 && scale <= 0;
    let decimal_exceeding_precision =
        jdbc_type == JdbcType::Decimal && precision > DEFAULT_PRECISION;
    if needs_resolving {
        resolved_precision = DEFAULT_PRECISION;
        resolved_scale = default_scale;
    } else if decimal_exceeding_precision {
        resolved_precision = DEFAULT_PRECISION;
    }

    match get_arrow_type_from_jdbc_type(jdbc_type, resolved_precision, resolved_scale, None) {
        Ok(DataType::Date32 | DataType::Date64) => Ok(Some(DataType::Date32)),
        Ok(DataType::Timestamp(_, _)) => Ok(Some(DataType::Date64)),
        Ok(arrow_type) => Ok(Some(arrow_type)),
        Err(FederationError::UnsupportedOperation(message)) => {
            tracing::warn!(
                "Error converting JDBC Type [{}] to arrow: {}",
                jdbc_type.code(),
                message
            );
            if jdbc_type == JdbcType::TimestampWithTimezone {
                tracing::debug!(
                    "Converting JDBC Type [{}] to arrow: {}",
                    jdbc_type.code(),
                    message
                );
                return Ok(Some(DataType::Date64));
            }
            Ok(None)
        }
        Err(other) => Err(other),
    }
}
