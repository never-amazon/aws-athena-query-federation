use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, IntervalUnit, TimeUnit, UnionFields, UnionMode};

use crate::data::ArrowType;
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{
    deserialize_nullable, get_next_bool_field, get_next_int_field, get_next_string_field, get_type,
    no_enum_constant, no_serde_configured, serialize_nullable, write_type,
};
use crate::serde::json::{JsonGenerator, JsonParser, JsonToken};

pub const SERDE_SUB_TYPES: &[&str] = &[
    "Null",
    "Struct",
    "List",
    "FixedSizeList",
    "Union",
    "Int",
    "FloatingPoint",
    "Utf8",
    "Binary",
    "FixedSizeBinary",
    "Bool",
    "Decimal",
    "Date",
    "Time",
    "Timestamp",
    "Interval",
    "Map",
];

const LIST_SIZE_FIELD: &str = "listSize";
const MODE_FIELD: &str = "mode";
const TYPE_IDS_FIELD: &str = "typeIds";
const BIT_WIDTH_FIELD: &str = "bitWidth";
const IS_SIGNED_FIELD: &str = "isSigned";
const PRECISION_FIELD: &str = "precision";
const BYTE_WIDTH_FIELD: &str = "byteWidth";
const SCALE_FIELD: &str = "scale";
const UNIT_FIELD: &str = "unit";
const TIMEZONE_FIELD: &str = "timezone";

pub fn java_type_name(arrow_type: &ArrowType) -> &'static str {
    match arrow_type {
        DataType::Null => "Null",
        DataType::Boolean => "Bool",
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => "Int",
        DataType::Float16 | DataType::Float32 | DataType::Float64 => "FloatingPoint",
        DataType::Timestamp(_, _) => "Timestamp",
        DataType::Date32 | DataType::Date64 => "Date",
        DataType::Time32(_) | DataType::Time64(_) => "Time",
        DataType::Duration(_) => "Duration",
        DataType::Interval(_) => "Interval",
        DataType::Binary => "Binary",
        DataType::FixedSizeBinary(_) => "FixedSizeBinary",
        DataType::LargeBinary => "LargeBinary",
        DataType::BinaryView => "BinaryView",
        DataType::Utf8 => "Utf8",
        DataType::LargeUtf8 => "LargeUtf8",
        DataType::Utf8View => "Utf8View",
        DataType::List(_) => "List",
        DataType::ListView(_) => "ListView",
        DataType::FixedSizeList(_, _) => "FixedSizeList",
        DataType::LargeList(_) => "LargeList",
        DataType::LargeListView(_) => "LargeListView",
        DataType::Struct(_) => "Struct",
        DataType::Union(_, _) => "Union",
        DataType::Dictionary(_, value) => java_type_name(value),
        DataType::Decimal32(_, _)
        | DataType::Decimal64(_, _)
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _) => "Decimal",
        DataType::Map(_, _) => "Map",
        DataType::RunEndEncoded(_, _) => "RunEndEncoded",
    }
}

pub fn serialize(arrow_type: Option<&ArrowType>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, arrow_type, do_serialize)
}

pub fn do_serialize(arrow_type: &ArrowType, jgen: &mut JsonGenerator) -> Result<()> {
    let type_name = java_type_name(arrow_type);
    if !SERDE_SUB_TYPES.contains(&type_name) {
        return Err(no_serde_configured(type_name));
    }
    write_type(jgen, type_name)?;
    match arrow_type {
        DataType::FixedSizeList(_, size) => jgen.write_number_field_i32(LIST_SIZE_FIELD, *size),
        DataType::Union(fields, mode) => {
            let ids: Vec<i32> = fields.iter().map(|(id, _)| i32::from(id)).collect();
            jgen.write_string_field(MODE_FIELD, Some(&union_to_string(*mode, &ids)))?;
            jgen.write_array_field_start(TYPE_IDS_FIELD)?;
            for id in ids {
                jgen.write_number_i32(id)?;
            }
            jgen.write_end_array()
        }
        DataType::Int8 => write_int(jgen, 8, true),
        DataType::Int16 => write_int(jgen, 16, true),
        DataType::Int32 => write_int(jgen, 32, true),
        DataType::Int64 => write_int(jgen, 64, true),
        DataType::UInt8 => write_int(jgen, 8, false),
        DataType::UInt16 => write_int(jgen, 16, false),
        DataType::UInt32 => write_int(jgen, 32, false),
        DataType::UInt64 => write_int(jgen, 64, false),
        DataType::Float16 => jgen.write_string_field(PRECISION_FIELD, Some("HALF")),
        DataType::Float32 => jgen.write_string_field(PRECISION_FIELD, Some("SINGLE")),
        DataType::Float64 => jgen.write_string_field(PRECISION_FIELD, Some("DOUBLE")),
        DataType::FixedSizeBinary(width) => jgen.write_number_field_i32(BYTE_WIDTH_FIELD, *width),
        DataType::Decimal32(p, s)
        | DataType::Decimal64(p, s)
        | DataType::Decimal128(p, s)
        | DataType::Decimal256(p, s) => {
            jgen.write_number_field_i32(PRECISION_FIELD, i32::from(*p))?;
            jgen.write_number_field_i32(SCALE_FIELD, i32::from(*s))
        }
        DataType::Date32 => jgen.write_string_field(UNIT_FIELD, Some("DAY")),
        DataType::Date64 => jgen.write_string_field(UNIT_FIELD, Some("MILLISECOND")),
        DataType::Time32(unit) => {
            jgen.write_string_field(UNIT_FIELD, Some(time_unit_name(unit)))?;
            jgen.write_number_field_i32(BIT_WIDTH_FIELD, 32)
        }
        DataType::Time64(unit) => {
            jgen.write_string_field(UNIT_FIELD, Some(time_unit_name(unit)))?;
            jgen.write_number_field_i32(BIT_WIDTH_FIELD, 64)
        }
        DataType::Timestamp(unit, tz) => {
            jgen.write_string_field(UNIT_FIELD, Some(time_unit_name(unit)))?;
            jgen.write_string_field(TIMEZONE_FIELD, tz.as_deref())
        }
        DataType::Interval(unit) => {
            jgen.write_string_field(UNIT_FIELD, Some(interval_unit_name(unit)))
        }
        _ => Ok(()),
    }
}

fn write_int(jgen: &mut JsonGenerator, bit_width: i32, signed: bool) -> Result<()> {
    jgen.write_number_field_i32(BIT_WIDTH_FIELD, bit_width)?;
    jgen.write_boolean_field(IS_SIGNED_FIELD, signed)
}

pub fn union_to_string(mode: UnionMode, type_ids: &[i32]) -> String {
    let ids: Vec<String> = type_ids
        .iter()
        .map(std::string::ToString::to_string)
        .collect();
    format!("Union({}, [{}])", union_mode_name(mode), ids.join(", "))
}

pub fn union_mode_name(mode: UnionMode) -> &'static str {
    match mode {
        UnionMode::Sparse => "Sparse",
        UnionMode::Dense => "Dense",
    }
}

pub fn time_unit_name(unit: &TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Second => "SECOND",
        TimeUnit::Millisecond => "MILLISECOND",
        TimeUnit::Microsecond => "MICROSECOND",
        TimeUnit::Nanosecond => "NANOSECOND",
    }
}

pub fn interval_unit_name(unit: &IntervalUnit) -> &'static str {
    match unit {
        IntervalUnit::YearMonth => "YEAR_MONTH",
        IntervalUnit::DayTime => "DAY_TIME",
        IntervalUnit::MonthDayNano => "MONTH_DAY_NANO",
    }
}

fn time_unit_value_of(name: Option<&str>) -> Result<TimeUnit> {
    match name {
        Some("SECOND") => Ok(TimeUnit::Second),
        Some("MILLISECOND") => Ok(TimeUnit::Millisecond),
        Some("MICROSECOND") => Ok(TimeUnit::Microsecond),
        Some("NANOSECOND") => Ok(TimeUnit::Nanosecond),
        other => Err(no_enum_constant(
            "org.apache.arrow.vector.types.TimeUnit",
            other,
        )),
    }
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<ArrowType>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<ArrowType> {
    let type_name = get_type(jparser)?;
    let name = type_name.as_deref().unwrap_or("null");
    if !SERDE_SUB_TYPES.contains(&name) {
        return Err(no_serde_configured(name));
    }
    do_typed_deserialize(name, jparser)
}

pub fn placeholder_child() -> Arc<Field> {
    Arc::new(Field::new("$data$", DataType::Null, true))
}

fn deserialize_union(jparser: &mut JsonParser<'_>) -> Result<ArrowType> {
    let mode_name = get_next_string_field(jparser, MODE_FIELD)?;
    let mode = match mode_name.as_deref() {
        Some("Sparse") => UnionMode::Sparse,
        Some("Dense") => UnionMode::Dense,
        other => {
            return Err(no_enum_constant(
                "org.apache.arrow.vector.types.UnionMode",
                other,
            ))
        }
    };
    crate::serde::base::assert_field_name(jparser, Some(TYPE_IDS_FIELD))?;
    let mut type_ids: Vec<i32> = Vec::new();
    loop {
        match jparser.next_token()? {
            Some(JsonToken::EndArray) => break,
            None => {
                return Err(FederationError::io(
                    "Unexpected end-of-input: expected close marker for ARRAY",
                ))
            }
            Some(_) => type_ids.push(jparser.get_value_as_int()?),
        }
    }
    let ids: Vec<i8> = type_ids
        .iter()
        .map(|i| {
            i8::try_from(*i).map_err(|_| {
                FederationError::IllegalArgument(format!("Union type id {i} out of range"))
            })
        })
        .collect::<Result<Vec<i8>>>()?;
    let fields: Vec<Field> = ids
        .iter()
        .map(|i| Field::new(format!("$data${i}"), DataType::Null, true))
        .collect();
    let union_fields = UnionFields::try_new(ids, fields)
        .map_err(|e| FederationError::IllegalArgument(e.to_string()))?;
    Ok(DataType::Union(union_fields, mode))
}

fn deserialize_decimal(jparser: &mut JsonParser<'_>) -> Result<ArrowType> {
    let precision = get_next_int_field(jparser, PRECISION_FIELD)?;
    let scale = get_next_int_field(jparser, SCALE_FIELD)?;
    let precision = u8::try_from(precision).map_err(|_| {
        FederationError::IllegalArgument(format!("Decimal precision {precision} out of range"))
    })?;
    let scale = i8::try_from(scale).map_err(|_| {
        FederationError::IllegalArgument(format!("Decimal scale {scale} out of range"))
    })?;
    Ok(DataType::Decimal128(precision, scale))
}

fn do_typed_deserialize(type_name: &str, jparser: &mut JsonParser<'_>) -> Result<ArrowType> {
    match type_name {
        "Null" => Ok(DataType::Null),
        "Struct" => Ok(DataType::Struct(Fields::empty())),
        "List" => Ok(DataType::List(placeholder_child())),
        "FixedSizeList" => {
            let list_size = get_next_int_field(jparser, LIST_SIZE_FIELD)?;
            Ok(DataType::FixedSizeList(placeholder_child(), list_size))
        }
        "Union" => deserialize_union(jparser),
        "Int" => {
            let bit_width = get_next_int_field(jparser, BIT_WIDTH_FIELD)?;
            let is_signed = get_next_bool_field(jparser, IS_SIGNED_FIELD)?;
            match (bit_width, is_signed) {
                (8, true) => Ok(DataType::Int8),
                (16, true) => Ok(DataType::Int16),
                (32, true) => Ok(DataType::Int32),
                (64, true) => Ok(DataType::Int64),
                (8, false) => Ok(DataType::UInt8),
                (16, false) => Ok(DataType::UInt16),
                (32, false) => Ok(DataType::UInt32),
                (64, false) => Ok(DataType::UInt64),
                _ => Err(FederationError::IllegalArgument(format!(
                    "Unsupported Int bitWidth {bit_width} isSigned {is_signed}"
                ))),
            }
        }
        "FloatingPoint" => {
            let precision = get_next_string_field(jparser, PRECISION_FIELD)?;
            match precision.as_deref() {
                Some("HALF") => Ok(DataType::Float16),
                Some("SINGLE") => Ok(DataType::Float32),
                Some("DOUBLE") => Ok(DataType::Float64),
                other => Err(no_enum_constant(
                    "org.apache.arrow.vector.types.FloatingPointPrecision",
                    other,
                )),
            }
        }
        "Utf8" => Ok(DataType::Utf8),
        "Binary" => Ok(DataType::Binary),
        "FixedSizeBinary" => {
            let byte_width = get_next_int_field(jparser, BYTE_WIDTH_FIELD)?;
            Ok(DataType::FixedSizeBinary(byte_width))
        }
        "Bool" => Ok(DataType::Boolean),
        "Decimal" => deserialize_decimal(jparser),
        "Date" => {
            let unit = get_next_string_field(jparser, UNIT_FIELD)?;
            match unit.as_deref() {
                Some("DAY") => Ok(DataType::Date32),
                Some("MILLISECOND") => Ok(DataType::Date64),
                other => Err(no_enum_constant(
                    "org.apache.arrow.vector.types.DateUnit",
                    other,
                )),
            }
        }
        "Time" => {
            let unit = time_unit_value_of(get_next_string_field(jparser, UNIT_FIELD)?.as_deref())?;
            let bit_width = get_next_int_field(jparser, BIT_WIDTH_FIELD)?;
            match (unit, bit_width) {
                (TimeUnit::Second | TimeUnit::Millisecond, 32) => Ok(DataType::Time32(unit)),
                (TimeUnit::Microsecond | TimeUnit::Nanosecond, 64) => Ok(DataType::Time64(unit)),
                (unit, width) => Err(FederationError::IllegalArgument(format!(
                    "Unsupported Time unit {} with bitWidth {}",
                    time_unit_name(&unit),
                    width
                ))),
            }
        }
        "Timestamp" => {
            let unit = time_unit_value_of(get_next_string_field(jparser, UNIT_FIELD)?.as_deref())?;
            let timezone = get_next_string_field(jparser, TIMEZONE_FIELD)?;
            Ok(DataType::Timestamp(
                unit,
                timezone.map(std::convert::Into::into),
            ))
        }
        "Interval" => {
            let unit = get_next_string_field(jparser, UNIT_FIELD)?;
            match unit.as_deref() {
                Some("YEAR_MONTH") => Ok(DataType::Interval(IntervalUnit::YearMonth)),
                Some("DAY_TIME") => Ok(DataType::Interval(IntervalUnit::DayTime)),
                Some("MONTH_DAY_NANO") => Ok(DataType::Interval(IntervalUnit::MonthDayNano)),
                other => Err(no_enum_constant(
                    "org.apache.arrow.vector.types.IntervalUnit",
                    other,
                )),
            }
        }
        "Map" => Ok(DataType::Map(
            Arc::new(Field::new(
                "entries",
                DataType::Struct(Fields::empty()),
                false,
            )),
            false,
        )),
        other => Err(no_serde_configured(other)),
    }
}

pub fn get_delegate_ser_de_map() -> Vec<&'static str> {
    SERDE_SUB_TYPES.to_vec()
}
