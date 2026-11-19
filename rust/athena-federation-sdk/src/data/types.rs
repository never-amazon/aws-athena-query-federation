use std::fmt;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef, Fields, IntervalUnit, TimeUnit, UnionMode};

use crate::exceptions::{FederationError, Result};

pub type ArrowType = DataType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MinorType {
    Null,
    Struct,
    TinyInt,
    SmallInt,
    Int,
    BigInt,
    DateDay,
    DateMilli,
    TimeSec,
    TimeMilli,
    TimeMicro,
    TimeNano,
    TimestampSec,
    TimestampMilli,
    TimestampMicro,
    TimestampNano,
    IntervalDay,
    IntervalMonthDayNano,
    Duration,
    IntervalYear,
    Float2,
    Float4,
    Float8,
    Bit,
    VarChar,
    ViewVarChar,
    LargeVarChar,
    LargeVarBinary,
    VarBinary,
    ViewVarBinary,
    Decimal,
    Decimal256,
    FixedSizeBinary,
    UInt1,
    UInt2,
    UInt4,
    UInt8,
    List,
    ListView,
    LargeList,
    FixedSizeList,
    Union,
    DenseUnion,
    Map,
    TimestampSecTz,
    TimestampMilliTz,
    TimestampMicroTz,
    TimestampNanoTz,
    ExtensionType,
}

impl fmt::Display for MinorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MinorType {
    pub const ALL: [MinorType; 49] = [
        MinorType::Null,
        MinorType::Struct,
        MinorType::TinyInt,
        MinorType::SmallInt,
        MinorType::Int,
        MinorType::BigInt,
        MinorType::DateDay,
        MinorType::DateMilli,
        MinorType::TimeSec,
        MinorType::TimeMilli,
        MinorType::TimeMicro,
        MinorType::TimeNano,
        MinorType::TimestampSec,
        MinorType::TimestampMilli,
        MinorType::TimestampMicro,
        MinorType::TimestampNano,
        MinorType::IntervalDay,
        MinorType::IntervalMonthDayNano,
        MinorType::Duration,
        MinorType::IntervalYear,
        MinorType::Float2,
        MinorType::Float4,
        MinorType::Float8,
        MinorType::Bit,
        MinorType::VarChar,
        MinorType::ViewVarChar,
        MinorType::LargeVarChar,
        MinorType::LargeVarBinary,
        MinorType::VarBinary,
        MinorType::ViewVarBinary,
        MinorType::Decimal,
        MinorType::Decimal256,
        MinorType::FixedSizeBinary,
        MinorType::UInt1,
        MinorType::UInt2,
        MinorType::UInt4,
        MinorType::UInt8,
        MinorType::List,
        MinorType::ListView,
        MinorType::LargeList,
        MinorType::FixedSizeList,
        MinorType::Union,
        MinorType::DenseUnion,
        MinorType::Map,
        MinorType::TimestampSecTz,
        MinorType::TimestampMilliTz,
        MinorType::TimestampMicroTz,
        MinorType::TimestampNanoTz,
        MinorType::ExtensionType,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MinorType::Null => "NULL",
            MinorType::Struct => "STRUCT",
            MinorType::TinyInt => "TINYINT",
            MinorType::SmallInt => "SMALLINT",
            MinorType::Int => "INT",
            MinorType::BigInt => "BIGINT",
            MinorType::DateDay => "DATEDAY",
            MinorType::DateMilli => "DATEMILLI",
            MinorType::TimeSec => "TIMESEC",
            MinorType::TimeMilli => "TIMEMILLI",
            MinorType::TimeMicro => "TIMEMICRO",
            MinorType::TimeNano => "TIMENANO",
            MinorType::TimestampSec => "TIMESTAMPSEC",
            MinorType::TimestampMilli => "TIMESTAMPMILLI",
            MinorType::TimestampMicro => "TIMESTAMPMICRO",
            MinorType::TimestampNano => "TIMESTAMPNANO",
            MinorType::IntervalDay => "INTERVALDAY",
            MinorType::IntervalMonthDayNano => "INTERVALMONTHDAYNANO",
            MinorType::Duration => "DURATION",
            MinorType::IntervalYear => "INTERVALYEAR",
            MinorType::Float2 => "FLOAT2",
            MinorType::Float4 => "FLOAT4",
            MinorType::Float8 => "FLOAT8",
            MinorType::Bit => "BIT",
            MinorType::VarChar => "VARCHAR",
            MinorType::ViewVarChar => "VIEWVARCHAR",
            MinorType::LargeVarChar => "LARGEVARCHAR",
            MinorType::LargeVarBinary => "LARGEVARBINARY",
            MinorType::VarBinary => "VARBINARY",
            MinorType::ViewVarBinary => "VIEWVARBINARY",
            MinorType::Decimal => "DECIMAL",
            MinorType::Decimal256 => "DECIMAL256",
            MinorType::FixedSizeBinary => "FIXEDSIZEBINARY",
            MinorType::UInt1 => "UINT1",
            MinorType::UInt2 => "UINT2",
            MinorType::UInt4 => "UINT4",
            MinorType::UInt8 => "UINT8",
            MinorType::List => "LIST",
            MinorType::ListView => "LISTVIEW",
            MinorType::LargeList => "LARGELIST",
            MinorType::FixedSizeList => "FIXED_SIZE_LIST",
            MinorType::Union => "UNION",
            MinorType::DenseUnion => "DENSEUNION",
            MinorType::Map => "MAP",
            MinorType::TimestampSecTz => "TIMESTAMPSECTZ",
            MinorType::TimestampMilliTz => "TIMESTAMPMILLITZ",
            MinorType::TimestampMicroTz => "TIMESTAMPMICROTZ",
            MinorType::TimestampNanoTz => "TIMESTAMPNANOTZ",
            MinorType::ExtensionType => "EXTENSIONTYPE",
        }
    }
}

pub fn minor_type_of(arrow_type: &ArrowType) -> MinorType {
    match arrow_type {
        DataType::Null => MinorType::Null,
        DataType::Struct(_) => MinorType::Struct,
        DataType::List(_) => MinorType::List,
        DataType::ListView(_) | DataType::LargeListView(_) => MinorType::ListView,
        DataType::LargeList(_) => MinorType::LargeList,
        DataType::FixedSizeList(_, _) => MinorType::FixedSizeList,
        DataType::Union(_, UnionMode::Sparse) => MinorType::Union,
        DataType::Union(_, UnionMode::Dense) => MinorType::DenseUnion,
        DataType::Map(_, _) => MinorType::Map,
        DataType::Int8 => MinorType::TinyInt,
        DataType::Int16 => MinorType::SmallInt,
        DataType::Int32 => MinorType::Int,
        DataType::Int64 => MinorType::BigInt,
        DataType::UInt8 => MinorType::UInt1,
        DataType::UInt16 => MinorType::UInt2,
        DataType::UInt32 => MinorType::UInt4,
        DataType::UInt64 => MinorType::UInt8,
        DataType::Float16 => MinorType::Float2,
        DataType::Float32 => MinorType::Float4,
        DataType::Float64 => MinorType::Float8,
        DataType::Utf8 => MinorType::VarChar,
        DataType::LargeUtf8 => MinorType::LargeVarChar,
        DataType::Utf8View => MinorType::ViewVarChar,
        DataType::Binary => MinorType::VarBinary,
        DataType::LargeBinary => MinorType::LargeVarBinary,
        DataType::BinaryView => MinorType::ViewVarBinary,
        DataType::FixedSizeBinary(_) => MinorType::FixedSizeBinary,
        DataType::Boolean => MinorType::Bit,
        DataType::Decimal32(_, _) | DataType::Decimal64(_, _) | DataType::Decimal128(_, _) => {
            MinorType::Decimal
        }
        DataType::Decimal256(_, _) => MinorType::Decimal256,
        DataType::Date32 => MinorType::DateDay,
        DataType::Date64 => MinorType::DateMilli,
        DataType::Time32(TimeUnit::Second) => MinorType::TimeSec,
        DataType::Time32(_) => MinorType::TimeMilli,
        DataType::Time64(TimeUnit::Nanosecond) => MinorType::TimeNano,
        DataType::Time64(_) => MinorType::TimeMicro,
        DataType::Timestamp(unit, tz) => match (unit, tz.is_some()) {
            (TimeUnit::Second, false) => MinorType::TimestampSec,
            (TimeUnit::Millisecond, false) => MinorType::TimestampMilli,
            (TimeUnit::Microsecond, false) => MinorType::TimestampMicro,
            (TimeUnit::Nanosecond, false) => MinorType::TimestampNano,
            (TimeUnit::Second, true) => MinorType::TimestampSecTz,
            (TimeUnit::Millisecond, true) => MinorType::TimestampMilliTz,
            (TimeUnit::Microsecond, true) => MinorType::TimestampMicroTz,
            (TimeUnit::Nanosecond, true) => MinorType::TimestampNanoTz,
        },
        DataType::Interval(IntervalUnit::DayTime) => MinorType::IntervalDay,
        DataType::Interval(IntervalUnit::YearMonth) => MinorType::IntervalYear,
        DataType::Interval(IntervalUnit::MonthDayNano) => MinorType::IntervalMonthDayNano,
        DataType::Duration(_) => MinorType::Duration,
        DataType::Dictionary(_, value) => minor_type_of(value),
        DataType::RunEndEncoded(_, _) => MinorType::ExtensionType,
    }
}

impl MinorType {
    pub fn get_type(&self) -> Result<ArrowType> {
        Ok(match self {
            MinorType::Null => DataType::Null,
            MinorType::Struct => DataType::Struct(Fields::empty()),
            MinorType::TinyInt => DataType::Int8,
            MinorType::SmallInt => DataType::Int16,
            MinorType::Int => DataType::Int32,
            MinorType::BigInt => DataType::Int64,
            MinorType::DateDay => DataType::Date32,
            MinorType::DateMilli => DataType::Date64,
            MinorType::TimeSec => DataType::Time32(TimeUnit::Second),
            MinorType::TimeMilli => DataType::Time32(TimeUnit::Millisecond),
            MinorType::TimeMicro => DataType::Time64(TimeUnit::Microsecond),
            MinorType::TimeNano => DataType::Time64(TimeUnit::Nanosecond),
            MinorType::TimestampSec => DataType::Timestamp(TimeUnit::Second, None),
            MinorType::TimestampMilli => DataType::Timestamp(TimeUnit::Millisecond, None),
            MinorType::TimestampMicro => DataType::Timestamp(TimeUnit::Microsecond, None),
            MinorType::TimestampNano => DataType::Timestamp(TimeUnit::Nanosecond, None),
            MinorType::IntervalDay => DataType::Interval(IntervalUnit::DayTime),
            MinorType::IntervalMonthDayNano => DataType::Interval(IntervalUnit::MonthDayNano),
            MinorType::IntervalYear => DataType::Interval(IntervalUnit::YearMonth),
            MinorType::Float2 => DataType::Float16,
            MinorType::Float4 => DataType::Float32,
            MinorType::Float8 => DataType::Float64,
            MinorType::Bit => DataType::Boolean,
            MinorType::VarChar => DataType::Utf8,
            MinorType::ViewVarChar => DataType::Utf8View,
            MinorType::LargeVarChar => DataType::LargeUtf8,
            MinorType::LargeVarBinary => DataType::LargeBinary,
            MinorType::VarBinary => DataType::Binary,
            MinorType::ViewVarBinary => DataType::BinaryView,
            MinorType::UInt1 => DataType::UInt8,
            MinorType::UInt2 => DataType::UInt16,
            MinorType::UInt4 => DataType::UInt32,
            MinorType::UInt8 => DataType::UInt64,
            MinorType::List => DataType::List(Arc::new(Field::new("$data$", DataType::Null, true))),
            MinorType::LargeList => {
                DataType::LargeList(Arc::new(Field::new("$data$", DataType::Null, true)))
            }
            other => {
                return Err(FederationError::UnsupportedOperation(format!(
                    "Cannot get simple type for type {other}"
                )))
            }
        })
    }
}

pub fn get_children(field: &Field) -> Vec<FieldRef> {
    match field.data_type() {
        DataType::Struct(fields) => fields.iter().cloned().collect(),
        DataType::List(child)
        | DataType::LargeList(child)
        | DataType::ListView(child)
        | DataType::LargeListView(child)
        | DataType::FixedSizeList(child, _)
        | DataType::Map(child, _) => vec![child.clone()],
        DataType::Union(fields, _) => fields.iter().map(|(_, f)| f.clone()).collect(),
        _ => Vec::new(),
    }
}

pub fn with_children(arrow_type: &ArrowType, children: Vec<FieldRef>) -> ArrowType {
    match arrow_type {
        DataType::Struct(_) => DataType::Struct(Fields::from(children)),
        DataType::List(old) => {
            DataType::List(children.into_iter().next().unwrap_or_else(|| old.clone()))
        }
        DataType::LargeList(old) => {
            DataType::LargeList(children.into_iter().next().unwrap_or_else(|| old.clone()))
        }
        DataType::FixedSizeList(old, n) => DataType::FixedSizeList(
            children.into_iter().next().unwrap_or_else(|| old.clone()),
            *n,
        ),
        DataType::Map(old, sorted) => DataType::Map(
            children.into_iter().next().unwrap_or_else(|| old.clone()),
            *sorted,
        ),
        other => other.clone(),
    }
}

pub fn new_field(
    name: impl Into<String>,
    nullable: bool,
    arrow_type: ArrowType,
    children: Option<Vec<FieldRef>>,
) -> Field {
    let dt = match children {
        Some(c) if !c.is_empty() => with_children(&arrow_type, c),
        _ => arrow_type,
    };
    Field::new(name, dt, nullable)
}

pub fn nullable_field(
    name: impl Into<String>,
    arrow_type: ArrowType,
    children: Option<Vec<FieldRef>>,
) -> Field {
    new_field(name, true, arrow_type, children)
}

pub fn map_type(keys_sorted: bool, key: Field, value: Field) -> ArrowType {
    let entries = Field::new(
        "entries",
        DataType::Struct(Fields::from(vec![key, value])),
        false,
    );
    DataType::Map(Arc::new(entries), keys_sorted)
}

pub fn decimal(precision: u8, scale: i8) -> ArrowType {
    DataType::Decimal128(precision, scale)
}

pub fn timestamp_tz(unit: TimeUnit, tz: &str) -> ArrowType {
    DataType::Timestamp(unit, Some(Arc::from(tz)))
}

pub fn decimal_precision_scale(arrow_type: &ArrowType) -> Option<(u8, i8)> {
    match arrow_type {
        DataType::Decimal32(p, s)
        | DataType::Decimal64(p, s)
        | DataType::Decimal128(p, s)
        | DataType::Decimal256(p, s) => Some((*p, *s)),
        _ => None,
    }
}

pub fn timestamp_timezone(arrow_type: &ArrowType) -> Option<&str> {
    match arrow_type {
        DataType::Timestamp(_, Some(tz)) => Some(tz.as_ref()),
        _ => None,
    }
}

fn time_unit_name(unit: TimeUnit) -> &'static str {
    match unit {
        TimeUnit::Second => "SECOND",
        TimeUnit::Millisecond => "MILLISECOND",
        TimeUnit::Microsecond => "MICROSECOND",
        TimeUnit::Nanosecond => "NANOSECOND",
    }
}

pub(crate) fn java_field_to_string(field: &Field) -> String {
    let mut text = format!(
        "{}: {}",
        field.name(),
        arrow_type_to_string(field.data_type())
    );
    let children = get_children(field);
    if !children.is_empty() {
        let children: Vec<String> = children.iter().map(|c| java_field_to_string(c)).collect();
        text.push('<');
        text.push_str(&children.join(", "));
        text.push('>');
    }
    if !field.is_nullable() {
        text.push_str(" not null");
    }
    text
}

pub fn arrow_type_to_string(arrow_type: &ArrowType) -> String {
    match arrow_type {
        DataType::Null => "Null".into(),
        DataType::Boolean => "Bool".into(),
        DataType::Int8 => "Int(8, true)".into(),
        DataType::Int16 => "Int(16, true)".into(),
        DataType::Int32 => "Int(32, true)".into(),
        DataType::Int64 => "Int(64, true)".into(),
        DataType::UInt8 => "Int(8, false)".into(),
        DataType::UInt16 => "Int(16, false)".into(),
        DataType::UInt32 => "Int(32, false)".into(),
        DataType::UInt64 => "Int(64, false)".into(),
        DataType::Float16 => "FloatingPoint(HALF)".into(),
        DataType::Float32 => "FloatingPoint(SINGLE)".into(),
        DataType::Float64 => "FloatingPoint(DOUBLE)".into(),
        DataType::Utf8 => "Utf8".into(),
        DataType::LargeUtf8 => "LargeUtf8".into(),
        DataType::Binary => "Binary".into(),
        DataType::LargeBinary => "LargeBinary".into(),
        DataType::FixedSizeBinary(n) => format!("FixedSizeBinary({n})"),
        DataType::Decimal32(p, s) => format!("Decimal({p}, {s}, 32)"),
        DataType::Decimal64(p, s) => format!("Decimal({p}, {s}, 64)"),
        DataType::Decimal128(p, s) => format!("Decimal({p}, {s}, 128)"),
        DataType::Decimal256(p, s) => format!("Decimal({p}, {s}, 256)"),
        DataType::Date32 => "Date(DAY)".into(),
        DataType::Date64 => "Date(MILLISECOND)".into(),
        DataType::Timestamp(unit, tz) => format!(
            "Timestamp({}, {})",
            time_unit_name(*unit),
            tz.as_ref()
                .map_or_else(|| "null".into(), std::string::ToString::to_string)
        ),
        DataType::Time32(unit) => format!("Time({}, 32)", time_unit_name(*unit)),
        DataType::Time64(unit) => format!("Time({}, 64)", time_unit_name(*unit)),
        DataType::Duration(unit) => format!("Duration({})", time_unit_name(*unit)),
        DataType::Interval(IntervalUnit::DayTime) => "Interval(DAY_TIME)".into(),
        DataType::Interval(IntervalUnit::YearMonth) => "Interval(YEAR_MONTH)".into(),
        DataType::Interval(IntervalUnit::MonthDayNano) => "Interval(MONTH_DAY_NANO)".into(),
        DataType::List(_) => "List".into(),
        DataType::LargeList(_) => "LargeList".into(),
        DataType::FixedSizeList(_, n) => format!("FixedSizeList({n})"),
        DataType::Struct(_) => "Struct".into(),
        DataType::Map(_, sorted) => format!("Map({sorted})"),
        other => format!("{other:?}"),
    }
}
