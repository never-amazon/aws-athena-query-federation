use std::collections::HashMap;

use arrow::datatypes::{DataType, TimeUnit};
use athena_jdbc::manager::{jdbc_to_arrow_type, DEFAULT_PRECISION};
use athena_jdbc::sql::Types;

fn convert(jdbc_type: i32, precision: i32, scale: i32) -> DataType {
    jdbc_to_arrow_type(jdbc_type, precision, scale, &HashMap::new())
        .unwrap()
        .unwrap()
}

#[test]
fn to_arrow_type() {
    assert_eq!(DataType::Boolean, convert(Types::BIT, 0, 0));
    assert_eq!(DataType::Boolean, convert(Types::BOOLEAN, 0, 0));
    assert_eq!(DataType::Int8, convert(Types::TINYINT, 0, 0));
    assert_eq!(DataType::Int16, convert(Types::SMALLINT, 0, 0));
    assert_eq!(DataType::Int32, convert(Types::INTEGER, 0, 0));
    assert_eq!(DataType::Int64, convert(Types::BIGINT, 0, 0));
    assert_eq!(DataType::Float32, convert(Types::REAL, 0, 0));
    assert_eq!(DataType::Float32, convert(Types::FLOAT, 0, 0));
    assert_eq!(DataType::Float64, convert(Types::DOUBLE, 0, 0));
    assert_eq!(DataType::Decimal128(5, 3), convert(Types::DECIMAL, 5, 3));
    assert_eq!(DataType::Decimal128(38, 0), convert(Types::NUMERIC, 0, 0));
    for jdbc_type in [
        Types::CHAR,
        Types::NCHAR,
        Types::VARCHAR,
        Types::NVARCHAR,
        Types::LONGVARCHAR,
        Types::LONGNVARCHAR,
    ] {
        assert_eq!(DataType::Utf8, convert(jdbc_type, 0, 0));
    }
    for jdbc_type in [Types::BINARY, Types::VARBINARY, Types::LONGVARBINARY] {
        assert_eq!(DataType::Binary, convert(jdbc_type, 0, 0));
    }
    assert_eq!(DataType::Date32, convert(Types::DATE, 0, 0));
    assert_eq!(
        DataType::Time32(TimeUnit::Millisecond),
        convert(Types::TIME, 0, 0)
    );
    assert_eq!(DataType::Date64, convert(Types::TIMESTAMP, 0, 0));
    assert!(matches!(convert(Types::ARRAY, 0, 0), DataType::List(_)));
    assert_eq!(
        DataType::Date64,
        convert(Types::TIMESTAMP_WITH_TIMEZONE, 0, 0)
    );
    let default_precision = u8::try_from(DEFAULT_PRECISION).unwrap();
    assert_eq!(
        DataType::Decimal128(default_precision, 3),
        convert(Types::DECIMAL, DEFAULT_PRECISION + 1, 3)
    );
    assert_eq!(
        DataType::Decimal128(default_precision, 0),
        convert(Types::NUMERIC, 0, -1)
    );
}
