use std::collections::HashMap;

use arrow::datatypes::{DataType, TimeUnit};
use athena_jdbc::manager::{jdbc_to_arrow_type, DEFAULT_PRECISION};
use athena_jdbc::sql::JdbcType;

fn convert(jdbc_type: JdbcType, precision: i32, scale: i32) -> DataType {
    jdbc_to_arrow_type(jdbc_type, precision, scale, &HashMap::new())
        .unwrap()
        .unwrap()
}

#[test]
fn to_arrow_type() {
    assert_eq!(DataType::Boolean, convert(JdbcType::Bit, 0, 0));
    assert_eq!(DataType::Boolean, convert(JdbcType::Boolean, 0, 0));
    assert_eq!(DataType::Int8, convert(JdbcType::TinyInt, 0, 0));
    assert_eq!(DataType::Int16, convert(JdbcType::SmallInt, 0, 0));
    assert_eq!(DataType::Int32, convert(JdbcType::Integer, 0, 0));
    assert_eq!(DataType::Int64, convert(JdbcType::BigInt, 0, 0));
    assert_eq!(DataType::Float32, convert(JdbcType::Real, 0, 0));
    assert_eq!(DataType::Float32, convert(JdbcType::Float, 0, 0));
    assert_eq!(DataType::Float64, convert(JdbcType::Double, 0, 0));
    assert_eq!(DataType::Decimal128(5, 3), convert(JdbcType::Decimal, 5, 3));
    assert_eq!(
        DataType::Decimal128(38, 0),
        convert(JdbcType::Numeric, 0, 0)
    );
    for jdbc_type in [
        JdbcType::Char,
        JdbcType::NChar,
        JdbcType::Varchar,
        JdbcType::NVarchar,
        JdbcType::LongVarchar,
        JdbcType::LongNVarchar,
    ] {
        assert_eq!(DataType::Utf8, convert(jdbc_type, 0, 0));
    }
    for jdbc_type in [
        JdbcType::Binary,
        JdbcType::VarBinary,
        JdbcType::LongVarBinary,
    ] {
        assert_eq!(DataType::Binary, convert(jdbc_type, 0, 0));
    }
    assert_eq!(DataType::Date32, convert(JdbcType::Date, 0, 0));
    assert_eq!(
        DataType::Time32(TimeUnit::Millisecond),
        convert(JdbcType::Time, 0, 0)
    );
    assert_eq!(DataType::Date64, convert(JdbcType::Timestamp, 0, 0));
    assert!(matches!(convert(JdbcType::Array, 0, 0), DataType::List(_)));
    assert_eq!(
        DataType::Date64,
        convert(JdbcType::TimestampWithTimezone, 0, 0)
    );
    let default_precision = u8::try_from(DEFAULT_PRECISION).unwrap();
    assert_eq!(
        DataType::Decimal128(default_precision, 3),
        convert(JdbcType::Decimal, DEFAULT_PRECISION + 1, 3)
    );
    assert_eq!(
        DataType::Decimal128(default_precision, 0),
        convert(JdbcType::Numeric, 0, -1)
    );
}
