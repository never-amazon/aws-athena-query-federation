use std::fmt;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef, Fields, IntervalUnit, TimeUnit, UnionMode};

use crate::exceptions::{FederationError, Result};

pub type ArrowType = DataType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MinorType {
    NULL,
    STRUCT,
    TINYINT,
    SMALLINT,
    INT,
    BIGINT,
    DATEDAY,
    DATEMILLI,
    TIMESEC,
    TIMEMILLI,
    TIMEMICRO,
    TIMENANO,
    TIMESTAMPSEC,
    TIMESTAMPMILLI,
    TIMESTAMPMICRO,
    TIMESTAMPNANO,
    INTERVALDAY,
    INTERVALMONTHDAYNANO,
    DURATION,
    INTERVALYEAR,
    FLOAT2,
    FLOAT4,
    FLOAT8,
    BIT,
    VARCHAR,
    VIEWVARCHAR,
    LARGEVARCHAR,
    LARGEVARBINARY,
    VARBINARY,
    VIEWVARBINARY,
    DECIMAL,
    DECIMAL256,
    FIXEDSIZEBINARY,
    UINT1,
    UINT2,
    UINT4,
    UINT8,
    LIST,
    LISTVIEW,
    LARGELIST,
    #[allow(non_camel_case_types, reason = "mirrors the Java MinorType constant")]
    FIXED_SIZE_LIST,
    UNION,
    DENSEUNION,
    MAP,
    TIMESTAMPSECTZ,
    TIMESTAMPMILLITZ,
    TIMESTAMPMICROTZ,
    TIMESTAMPNANOTZ,
    EXTENSIONTYPE,
}

impl fmt::Display for MinorType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl MinorType {
    pub fn get_minor_type_for_arrow_type(arrow_type: &ArrowType) -> MinorType {
        use MinorType::{
            BIGINT, BIT, DATEDAY, DATEMILLI, DECIMAL, DECIMAL256, DENSEUNION, DURATION,
            EXTENSIONTYPE, FIXEDSIZEBINARY, FIXED_SIZE_LIST, FLOAT2, FLOAT4, FLOAT8, INT,
            INTERVALDAY, INTERVALMONTHDAYNANO, INTERVALYEAR, LARGELIST, LARGEVARBINARY,
            LARGEVARCHAR, LIST, LISTVIEW, MAP, NULL, SMALLINT, STRUCT, TIMEMICRO, TIMEMILLI,
            TIMENANO, TIMESEC, TIMESTAMPMICRO, TIMESTAMPMICROTZ, TIMESTAMPMILLI, TIMESTAMPMILLITZ,
            TIMESTAMPNANO, TIMESTAMPNANOTZ, TIMESTAMPSEC, TIMESTAMPSECTZ, TINYINT, UINT1, UINT2,
            UINT4, UINT8, UNION, VARBINARY, VARCHAR, VIEWVARBINARY, VIEWVARCHAR,
        };
        match arrow_type {
            DataType::Null => NULL,
            DataType::Struct(_) => STRUCT,
            DataType::List(_) => LIST,
            DataType::ListView(_) | DataType::LargeListView(_) => LISTVIEW,
            DataType::LargeList(_) => LARGELIST,
            DataType::FixedSizeList(_, _) => FIXED_SIZE_LIST,
            DataType::Union(_, UnionMode::Sparse) => UNION,
            DataType::Union(_, UnionMode::Dense) => DENSEUNION,
            DataType::Map(_, _) => MAP,
            DataType::Int8 => TINYINT,
            DataType::Int16 => SMALLINT,
            DataType::Int32 => INT,
            DataType::Int64 => BIGINT,
            DataType::UInt8 => UINT1,
            DataType::UInt16 => UINT2,
            DataType::UInt32 => UINT4,
            DataType::UInt64 => UINT8,
            DataType::Float16 => FLOAT2,
            DataType::Float32 => FLOAT4,
            DataType::Float64 => FLOAT8,
            DataType::Utf8 => VARCHAR,
            DataType::LargeUtf8 => LARGEVARCHAR,
            DataType::Utf8View => VIEWVARCHAR,
            DataType::Binary => VARBINARY,
            DataType::LargeBinary => LARGEVARBINARY,
            DataType::BinaryView => VIEWVARBINARY,
            DataType::FixedSizeBinary(_) => FIXEDSIZEBINARY,
            DataType::Boolean => BIT,
            DataType::Decimal32(_, _) | DataType::Decimal64(_, _) | DataType::Decimal128(_, _) => {
                DECIMAL
            }
            DataType::Decimal256(_, _) => DECIMAL256,
            DataType::Date32 => DATEDAY,
            DataType::Date64 => DATEMILLI,
            DataType::Time32(TimeUnit::Second) => TIMESEC,
            DataType::Time32(_) => TIMEMILLI,
            DataType::Time64(TimeUnit::Nanosecond) => TIMENANO,
            DataType::Time64(_) => TIMEMICRO,
            DataType::Timestamp(unit, tz) => match (unit, tz.is_some()) {
                (TimeUnit::Second, false) => TIMESTAMPSEC,
                (TimeUnit::Millisecond, false) => TIMESTAMPMILLI,
                (TimeUnit::Microsecond, false) => TIMESTAMPMICRO,
                (TimeUnit::Nanosecond, false) => TIMESTAMPNANO,
                (TimeUnit::Second, true) => TIMESTAMPSECTZ,
                (TimeUnit::Millisecond, true) => TIMESTAMPMILLITZ,
                (TimeUnit::Microsecond, true) => TIMESTAMPMICROTZ,
                (TimeUnit::Nanosecond, true) => TIMESTAMPNANOTZ,
            },
            DataType::Interval(IntervalUnit::DayTime) => INTERVALDAY,
            DataType::Interval(IntervalUnit::YearMonth) => INTERVALYEAR,
            DataType::Interval(IntervalUnit::MonthDayNano) => INTERVALMONTHDAYNANO,
            DataType::Duration(_) => DURATION,
            DataType::Dictionary(_, value) => MinorType::get_minor_type_for_arrow_type(value),
            DataType::RunEndEncoded(_, _) => EXTENSIONTYPE,
        }
    }

    pub fn get_type(&self) -> Result<ArrowType> {
        use MinorType::{
            BIGINT, BIT, DATEDAY, DATEMILLI, FLOAT2, FLOAT4, FLOAT8, INT, INTERVALDAY,
            INTERVALMONTHDAYNANO, INTERVALYEAR, LARGELIST, LARGEVARBINARY, LARGEVARCHAR, LIST,
            NULL, SMALLINT, STRUCT, TIMEMICRO, TIMEMILLI, TIMENANO, TIMESEC, TIMESTAMPMICRO,
            TIMESTAMPMILLI, TIMESTAMPNANO, TIMESTAMPSEC, TINYINT, UINT1, UINT2, UINT4, UINT8,
            VARBINARY, VARCHAR, VIEWVARBINARY, VIEWVARCHAR,
        };
        Ok(match self {
            NULL => DataType::Null,
            STRUCT => DataType::Struct(Fields::empty()),
            TINYINT => DataType::Int8,
            SMALLINT => DataType::Int16,
            INT => DataType::Int32,
            BIGINT => DataType::Int64,
            DATEDAY => DataType::Date32,
            DATEMILLI => DataType::Date64,
            TIMESEC => DataType::Time32(TimeUnit::Second),
            TIMEMILLI => DataType::Time32(TimeUnit::Millisecond),
            TIMEMICRO => DataType::Time64(TimeUnit::Microsecond),
            TIMENANO => DataType::Time64(TimeUnit::Nanosecond),
            TIMESTAMPSEC => DataType::Timestamp(TimeUnit::Second, None),
            TIMESTAMPMILLI => DataType::Timestamp(TimeUnit::Millisecond, None),
            TIMESTAMPMICRO => DataType::Timestamp(TimeUnit::Microsecond, None),
            TIMESTAMPNANO => DataType::Timestamp(TimeUnit::Nanosecond, None),
            INTERVALDAY => DataType::Interval(IntervalUnit::DayTime),
            INTERVALMONTHDAYNANO => DataType::Interval(IntervalUnit::MonthDayNano),
            INTERVALYEAR => DataType::Interval(IntervalUnit::YearMonth),
            FLOAT2 => DataType::Float16,
            FLOAT4 => DataType::Float32,
            FLOAT8 => DataType::Float64,
            BIT => DataType::Boolean,
            VARCHAR => DataType::Utf8,
            VIEWVARCHAR => DataType::Utf8View,
            LARGEVARCHAR => DataType::LargeUtf8,
            LARGEVARBINARY => DataType::LargeBinary,
            VARBINARY => DataType::Binary,
            VIEWVARBINARY => DataType::BinaryView,
            UINT1 => DataType::UInt8,
            UINT2 => DataType::UInt16,
            UINT4 => DataType::UInt32,
            UINT8 => DataType::UInt64,
            LIST => DataType::List(Arc::new(Field::new("$data$", DataType::Null, true))),
            LARGELIST => DataType::LargeList(Arc::new(Field::new("$data$", DataType::Null, true))),
            other => {
                return Err(FederationError::UnsupportedOperation(format!(
                    "Cannot get simple type for type {other}"
                )))
            }
        })
    }
}

pub fn minor_type_of(arrow_type: &ArrowType) -> MinorType {
    MinorType::get_minor_type_for_arrow_type(arrow_type)
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
