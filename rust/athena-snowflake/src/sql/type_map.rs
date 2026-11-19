use athena_jdbc::sql::{ColumnMetadata, JdbcType, Nullability};
use serde_json::Value;

pub const EXTRA_TYPES_TIMESTAMP_LTZ: JdbcType = JdbcType::Vendor(50000);
pub const EXTRA_TYPES_TIMESTAMP_TZ: JdbcType = JdbcType::Vendor(50001);
pub const EXTRA_TYPES_TIMESTAMP_NTZ: JdbcType = JdbcType::Vendor(50002);
pub const EXTRA_TYPES_VECTOR: JdbcType = JdbcType::Vendor(50003);
pub const EXTRA_TYPES_DECFLOAT: JdbcType = JdbcType::Vendor(50004);
pub const EXTRA_TYPES_YEAR_MONTH_INTERVAL: JdbcType = JdbcType::Vendor(50005);
pub const EXTRA_TYPES_DAY_TIME_INTERVAL: JdbcType = JdbcType::Vendor(50006);

const DATE_STRING_LENGTH: i32 = 10;
const TIME_STRING_LENGTH: i32 = 18;
const TIMESTAMP_STRING_LENGTH: i32 = 35;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnowflakeColumnType {
    Fixed { precision: i32, scale: i32 },
    Real,
    Text { length: i32 },
    Boolean,
    Date,
    Time { scale: i32 },
    TimestampNtz { scale: i32 },
    TimestampLtz { scale: i32 },
    TimestampTz { scale: i32 },
    Variant,
    Object,
    Array,
    Binary { length: i32 },
    Geography,
    Geometry,
    Vector,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnowflakeColumn {
    pub name: String,
    pub column_type: SnowflakeColumnType,
    pub nullable: bool,
}

impl SnowflakeColumn {
    pub fn new(
        name: impl Into<String>,
        column_type: SnowflakeColumnType,
        nullable: bool,
    ) -> SnowflakeColumn {
        SnowflakeColumn {
            name: name.into(),
            column_type,
            nullable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeInfo {
    pub internal_type: JdbcType,
    pub type_name: String,
}

pub(crate) fn external_type(internal_type: JdbcType) -> JdbcType {
    match internal_type {
        EXTRA_TYPES_TIMESTAMP_LTZ => JdbcType::Timestamp,
        EXTRA_TYPES_TIMESTAMP_TZ => JdbcType::TimestampWithTimezone,
        other => other,
    }
}

pub(crate) fn snowflake_type(
    internal_name: &str,
    ext_type_name: Option<&str>,
    output_type: Option<&str>,
    scale: i32,
    is_structured: bool,
) -> TypeInfo {
    let named = |jdbc: JdbcType, default_name: &str| TypeInfo {
        internal_type: jdbc,
        type_name: ext_type_name.unwrap_or(default_name).to_string(),
    };
    match internal_name.to_uppercase().as_str() {
        "TEXT" => named(JdbcType::Varchar, "VARCHAR"),
        "CHAR" => named(JdbcType::Char, "CHAR"),
        "INTEGER" => named(JdbcType::Integer, "INTEGER"),
        "DECFLOAT" => TypeInfo {
            internal_type: JdbcType::Decimal,
            type_name: "DECFLOAT".to_string(),
        },
        "FIXED" => {
            let fixed = if scale == 0 {
                JdbcType::BigInt
            } else {
                JdbcType::Decimal
            };
            named(fixed, "NUMBER")
        }
        "REAL" => named(JdbcType::Double, "DOUBLE"),
        "TIMESTAMP" | "TIMESTAMP_LTZ" => named(EXTRA_TYPES_TIMESTAMP_LTZ, "TIMESTAMPLTZ"),
        "INTERVAL_YEAR_MONTH" => named(EXTRA_TYPES_YEAR_MONTH_INTERVAL, "INTERVAL_YEAR_MONTH"),
        "INTERVAL_DAY_TIME" => named(EXTRA_TYPES_DAY_TIME_INTERVAL, "INTERVAL_DAY_TIME"),
        "TIMESTAMP_NTZ" => named(JdbcType::Timestamp, "TIMESTAMPNTZ"),
        "TIMESTAMP_TZ" => named(EXTRA_TYPES_TIMESTAMP_TZ, "TIMESTAMPTZ"),
        "DATE" => named(JdbcType::Date, "DATE"),
        "TIME" => named(JdbcType::Time, "TIME"),
        "BOOLEAN" => named(JdbcType::Boolean, "BOOLEAN"),
        "VECTOR" => named(EXTRA_TYPES_VECTOR, "VECTOR"),
        "ARRAY" => named(
            if is_structured {
                JdbcType::Array
            } else {
                JdbcType::Varchar
            },
            "ARRAY",
        ),
        "MAP" => named(JdbcType::Struct, "OBJECT"),
        "OBJECT" => {
            let geo = matches!(ext_type_name, Some("GEOMETRY" | "GEOGRAPHY"));
            let jdbc = if is_structured && !geo {
                JdbcType::Struct
            } else {
                JdbcType::Varchar
            };
            named(jdbc, "OBJECT")
        }
        "VARIANT" => named(JdbcType::Varchar, "VARIANT"),
        "BINARY" => named(JdbcType::Binary, "BINARY"),
        "GEOGRAPHY" | "GEOMETRY" => {
            let jdbc = match output_type.map(str::to_uppercase).as_deref() {
                Some("BINARY") => JdbcType::Binary,
                _ => JdbcType::Varchar,
            };
            TypeInfo {
                internal_type: jdbc,
                type_name: internal_name.to_uppercase(),
            }
        }
        _ => TypeInfo {
            internal_type: JdbcType::Other,
            type_name: ext_type_name
                .map_or_else(|| internal_name.to_uppercase(), ToString::to_string),
        },
    }
}

impl SnowflakeColumnType {
    pub fn wire_name(&self) -> &str {
        match self {
            SnowflakeColumnType::Fixed { .. } => "fixed",
            SnowflakeColumnType::Real => "real",
            SnowflakeColumnType::Text { .. } => "text",
            SnowflakeColumnType::Boolean => "boolean",
            SnowflakeColumnType::Date => "date",
            SnowflakeColumnType::Time { .. } => "time",
            SnowflakeColumnType::TimestampNtz { .. } => "timestamp_ntz",
            SnowflakeColumnType::TimestampLtz { .. } => "timestamp_ltz",
            SnowflakeColumnType::TimestampTz { .. } => "timestamp_tz",
            SnowflakeColumnType::Variant => "variant",
            SnowflakeColumnType::Object => "object",
            SnowflakeColumnType::Array => "array",
            SnowflakeColumnType::Binary { .. } => "binary",
            SnowflakeColumnType::Geography => "geography",
            SnowflakeColumnType::Geometry => "geometry",
            SnowflakeColumnType::Vector => "vector",
            SnowflakeColumnType::Unknown(name) => name,
        }
    }

    pub(crate) fn precision(&self) -> i32 {
        match self {
            SnowflakeColumnType::Fixed { precision, .. } => *precision,
            _ => 0,
        }
    }

    pub(crate) fn scale(&self) -> i32 {
        match self {
            SnowflakeColumnType::Fixed { scale, .. }
            | SnowflakeColumnType::Time { scale }
            | SnowflakeColumnType::TimestampNtz { scale }
            | SnowflakeColumnType::TimestampLtz { scale }
            | SnowflakeColumnType::TimestampTz { scale } => *scale,
            _ => 0,
        }
    }

    pub(crate) fn length(&self) -> i32 {
        match self {
            SnowflakeColumnType::Text { length } | SnowflakeColumnType::Binary { length } => {
                *length
            }
            _ => 0,
        }
    }

    pub(crate) fn type_info(&self) -> TypeInfo {
        snowflake_type(self.wire_name(), None, None, self.scale(), false)
    }
}

fn result_precision(internal_type: JdbcType, column_type: &SnowflakeColumnType) -> i32 {
    match internal_type {
        JdbcType::Char | JdbcType::Varchar | JdbcType::Binary => column_type.length(),
        JdbcType::Integer | JdbcType::Decimal | JdbcType::BigInt => column_type.precision(),
        JdbcType::Date => DATE_STRING_LENGTH,
        JdbcType::Time => TIME_STRING_LENGTH,
        EXTRA_TYPES_TIMESTAMP_LTZ | EXTRA_TYPES_TIMESTAMP_TZ | JdbcType::Timestamp => {
            TIMESTAMP_STRING_LENGTH
        }
        _ => 0,
    }
}

fn result_display_size(internal_type: JdbcType, column_type: &SnowflakeColumnType) -> i32 {
    match internal_type {
        JdbcType::Char | JdbcType::Varchar | JdbcType::Binary => column_type.length(),
        JdbcType::Integer | JdbcType::BigInt => column_type.precision() + 1,
        JdbcType::Decimal => column_type.precision() + 2,
        JdbcType::Double => 24,
        JdbcType::Date => DATE_STRING_LENGTH,
        JdbcType::Time => TIME_STRING_LENGTH,
        EXTRA_TYPES_TIMESTAMP_LTZ | EXTRA_TYPES_TIMESTAMP_TZ | JdbcType::Timestamp => {
            TIMESTAMP_STRING_LENGTH
        }
        JdbcType::Boolean => 5,
        _ => 25,
    }
}

pub(crate) fn result_column_metadata(column: &SnowflakeColumn) -> ColumnMetadata {
    let info = column.column_type.type_info();
    let jdbc_type = external_type(info.internal_type);
    ColumnMetadata::new(column.name.clone(), jdbc_type)
        .with_type_name(info.type_name)
        .with_precision(result_precision(info.internal_type, &column.column_type))
        .with_scale(column.column_type.scale())
        .with_display_size(result_display_size(info.internal_type, &column.column_type))
        .with_nullability(if column.nullable {
            Nullability::Nullable
        } else {
            Nullability::NoNulls
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShowColumnType {
    pub jdbc_type: JdbcType,
    pub type_name: String,
    pub column_size: Option<i32>,
    pub scale: i32,
}

fn json_int(node: &Value, key: &str) -> i32 {
    match node.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or_default(),
        Some(Value::String(s)) => s.trim().parse().unwrap_or_default(),
        _ => 0,
    }
}

fn json_text<'a>(node: &'a Value, key: &str) -> Option<&'a str> {
    node.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

pub(crate) fn show_column_type(data_type: &Value) -> ShowColumnType {
    let precision = json_int(data_type, "precision");
    let scale = json_int(data_type, "scale");
    let length = json_int(data_type, "length");
    let dimension = {
        let dimension = json_int(data_type, "dimension");
        if dimension > 0 {
            dimension
        } else {
            json_int(data_type, "vectorDimension")
        }
    };
    let internal_name = json_text(data_type, "type").unwrap_or_default();
    let is_structured = data_type
        .get("fields")
        .and_then(Value::as_array)
        .is_some_and(|fields| !fields.is_empty());
    let info = snowflake_type(
        internal_name,
        json_text(data_type, "extTypeName"),
        json_text(data_type, "outputType"),
        scale,
        is_structured,
    );
    let column_size = match info.internal_type {
        JdbcType::Char | JdbcType::Varchar | JdbcType::Binary | JdbcType::VarBinary => Some(length),
        JdbcType::Decimal
        | JdbcType::Numeric
        | JdbcType::BigInt
        | JdbcType::Integer
        | JdbcType::SmallInt
        | JdbcType::TinyInt
        | JdbcType::Float
        | JdbcType::Double
        | JdbcType::Real
        | JdbcType::Date
        | JdbcType::Time
        | JdbcType::Timestamp
        | JdbcType::TimestampWithTimezone
        | EXTRA_TYPES_TIMESTAMP_LTZ
        | EXTRA_TYPES_TIMESTAMP_TZ
        | EXTRA_TYPES_TIMESTAMP_NTZ
        | EXTRA_TYPES_DECFLOAT => Some(precision),
        EXTRA_TYPES_VECTOR => Some(dimension),
        _ => None,
    };
    ShowColumnType {
        jdbc_type: external_type(info.internal_type),
        type_name: info.type_name,
        column_size,
        scale,
    }
}
