use std::fmt;

use crate::calcite::rel_data_type::RelDataType;
use crate::calcite::sql_type_name::SqlTypeName;

pub(crate) struct TypeText {
    body: String,
    nullable: bool,
}

impl TypeText {
    pub(crate) fn new(body: impl Into<String>, nullable: bool) -> TypeText {
        TypeText {
            body: body.into(),
            nullable,
        }
    }

    pub(crate) fn boolean(nullable: bool) -> TypeText {
        TypeText::new("boolean", nullable)
    }

    pub(crate) fn nullable(&self) -> bool {
        self.nullable
    }

    pub(crate) fn into_nullable(self) -> TypeText {
        TypeText {
            nullable: true,
            ..self
        }
    }
}

impl fmt::Display for TypeText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.body)?;
        if self.nullable {
            f.write_str("?")?;
        }
        Ok(())
    }
}

fn joined(items: &[TypeText]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn java_precision(ty: &RelDataType) -> i32 {
    ty.precision().unwrap_or(-1)
}

fn with_precision(name: &str, ty: &RelDataType) -> String {
    format!("{name}<{}>", java_precision(ty))
}

pub(crate) fn calcite_type_text(ty: &RelDataType) -> TypeText {
    let child =
        |t: Option<&RelDataType>| t.map_or_else(String::new, |c| calcite_type_text(c).to_string());
    let body = match ty.sql_type_name() {
        SqlTypeName::Boolean => "boolean".to_string(),
        SqlTypeName::Tinyint => "i8".to_string(),
        SqlTypeName::Smallint => "i16".to_string(),
        SqlTypeName::Integer => "i32".to_string(),
        SqlTypeName::Bigint => "i64".to_string(),
        SqlTypeName::Real => "fp32".to_string(),
        SqlTypeName::Float | SqlTypeName::Double => "fp64".to_string(),
        SqlTypeName::Varchar if ty.precision().is_none() => "string".to_string(),
        SqlTypeName::Varchar => with_precision("varchar", ty),
        SqlTypeName::Char => with_precision("char", ty),
        SqlTypeName::Varbinary => "binary".to_string(),
        SqlTypeName::Binary => with_precision("fixedbinary", ty),
        SqlTypeName::Date => "date".to_string(),
        SqlTypeName::Time => with_precision("precision_time", ty),
        SqlTypeName::Timestamp => with_precision("precision_timestamp", ty),
        SqlTypeName::TimestampWithLocalTimeZone => with_precision("precision_timestamp_tz", ty),
        SqlTypeName::IntervalYearMonth => "interval_year".to_string(),
        SqlTypeName::IntervalDaySecond => "interval_day".to_string(),
        SqlTypeName::Decimal => format!(
            "decimal<{},{}>",
            java_precision(ty),
            ty.scale().unwrap_or(i32::MIN)
        ),
        SqlTypeName::Row => {
            let fields: Vec<TypeText> = ty
                .field_list()
                .iter()
                .map(|f| calcite_type_text(f.ty()))
                .collect();
            format!("struct<{}>", joined(&fields))
        }
        SqlTypeName::Array => format!("list<{}>", child(ty.component_type())),
        SqlTypeName::Map => format!("map<{},{}>", child(ty.key_type()), child(ty.value_type())),
        _ => ty.to_string(),
    };
    TypeText::new(body, ty.is_nullable())
}
