use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};

use athena_federation_sdk::data::ArrowType;
use athena_federation_sdk::{FederationError, Result};

use crate::sql::{Nullability, ResultSetMetaData, Types};

const MAX_DECIMAL128_PRECISION: i32 = 38;

pub fn get_arrow_type_from_jdbc_type(
    jdbc_type: i32,
    precision: i32,
    scale: i32,
    time_zone: Option<&str>,
) -> Result<ArrowType> {
    let arrow_type = match jdbc_type {
        Types::BOOLEAN | Types::BIT => DataType::Boolean,
        Types::TINYINT => DataType::Int8,
        Types::SMALLINT => DataType::Int16,
        Types::INTEGER => DataType::Int32,
        Types::BIGINT => DataType::Int64,
        Types::NUMERIC | Types::DECIMAL => decimal_type(precision, scale),
        Types::REAL | Types::FLOAT => DataType::Float32,
        Types::DOUBLE => DataType::Float64,
        Types::CHAR
        | Types::NCHAR
        | Types::VARCHAR
        | Types::NVARCHAR
        | Types::LONGVARCHAR
        | Types::LONGNVARCHAR
        | Types::CLOB
        | Types::NCLOB => DataType::Utf8,
        Types::DATE => DataType::Date32,
        Types::TIME => DataType::Time32(TimeUnit::Millisecond),
        Types::TIMESTAMP => DataType::Timestamp(TimeUnit::Millisecond, time_zone.map(Into::into)),
        Types::BINARY | Types::VARBINARY | Types::LONGVARBINARY | Types::BLOB => DataType::Binary,
        Types::ARRAY => DataType::List(Field::new("$data$", DataType::Null, true).into()),
        Types::NULL => DataType::Null,
        Types::STRUCT => DataType::Struct(Fields::empty()),
        other => {
            return Err(FederationError::UnsupportedOperation(format!(
                "Unmapped JDBC type: {other}"
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
    for index in 1..=metadata.column_count() {
        let column = metadata.column(index)?;
        let arrow_type = get_arrow_type_from_jdbc_type(
            column.jdbc_type(),
            column.precision(),
            column.scale(),
            Some("UTC"),
        )?;
        if matches!(arrow_type, DataType::List(_)) {
            return Err(FederationError::IllegalArgument(format!(
                "Configuration does not provide a mapping for array column {index}"
            )));
        }
        let nullable = column.nullability() != Nullability::NoNulls;
        fields.push(Field::new(column.label(), arrow_type, nullable));
    }
    Ok(Schema::new(fields))
}
