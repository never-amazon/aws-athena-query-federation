use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use super::type_converter::type_nullable;
use crate::calcite::rel_data_type::RelDataType;
use crate::calcite::sql_type_name::SqlTypeName;
use crate::error::{Result, SubstraitError};
use crate::proto::expression::literal::LiteralType;
use crate::proto::expression::Literal;
use crate::proto::r#type::Kind;
use crate::proto::Type;

pub(crate) struct TypeText {
    body: String,
    nullable: bool,
}

impl TypeText {
    fn new(body: impl Into<String>, nullable: bool) -> TypeText {
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

fn kind_not_set() -> SubstraitError {
    SubstraitError::Unsupported("Unsupported type: KIND_NOT_SET".into())
}

fn user_defined(type_names: &HashMap<u32, Rc<str>>, reference: u32) -> String {
    format!("u!{}", type_names.get(&reference).map_or("", |n| &**n))
}

pub(crate) fn proto_type_text(ty: &Type, type_names: &HashMap<u32, Rc<str>>) -> Result<TypeText> {
    let child = |t: Option<&Type>| proto_type_text(t.ok_or_else(kind_not_set)?, type_names);
    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    let body = match ty.kind.as_ref().ok_or_else(kind_not_set)? {
        Kind::Bool(_) => "boolean".to_string(),
        Kind::I8(_) => "i8".to_string(),
        Kind::I16(_) => "i16".to_string(),
        Kind::I32(_) => "i32".to_string(),
        Kind::I64(_) => "i64".to_string(),
        Kind::Fp32(_) => "fp32".to_string(),
        Kind::Fp64(_) => "fp64".to_string(),
        Kind::String(_) => "string".to_string(),
        Kind::Binary(_) => "binary".to_string(),
        Kind::Timestamp(_) => "timestamp".to_string(),
        Kind::Date(_) => "date".to_string(),
        Kind::Time(_) => "time".to_string(),
        Kind::IntervalYear(_) => "interval_year".to_string(),
        Kind::IntervalDay(_) => "interval_day".to_string(),
        Kind::IntervalCompound(_) => "interval_compound".to_string(),
        Kind::TimestampTz(_) => "timestamp_tz".to_string(),
        Kind::Uuid(_) => "uuid".to_string(),
        Kind::FixedChar(c) => format!("char<{}>", c.length),
        Kind::Varchar(c) => format!("varchar<{}>", c.length),
        Kind::FixedBinary(c) => format!("fixedbinary<{}>", c.length),
        Kind::Decimal(d) => format!("decimal<{},{}>", d.precision, d.scale),
        Kind::PrecisionTime(p) => format!("precision_time<{}>", p.precision),
        Kind::PrecisionTimestamp(p) => format!("precision_timestamp<{}>", p.precision),
        Kind::PrecisionTimestampTz(p) => format!("precision_timestamp_tz<{}>", p.precision),
        Kind::Struct(s) => {
            let fields = s
                .types
                .iter()
                .map(|t| proto_type_text(t, type_names))
                .collect::<Result<Vec<_>>>()?;
            format!("struct<{}>", joined(&fields))
        }
        Kind::List(l) => format!("list<{}>", child(l.r#type.as_deref())?),
        Kind::Map(m) => format!(
            "map<{},{}>",
            child(m.key.as_deref())?,
            child(m.value.as_deref())?
        ),
        Kind::UserDefined(u) => user_defined(type_names, u.type_reference),
        Kind::UserDefinedTypeReference(_) => {
            return Err(SubstraitError::Unsupported(
                "Unsupported type: USER_DEFINED_TYPE_REFERENCE".into(),
            ))
        }
        #[allow(
            unreachable_patterns,
            reason = "keeps the Java default branch for proto kinds added later"
        )]
        _ => return Err(SubstraitError::Unsupported("Unsupported type".into())),
    };
    Ok(TypeText::new(body, type_nullable(ty)))
}

pub(crate) fn literal_type_text(
    literal: &Literal,
    type_names: &HashMap<u32, Rc<str>>,
) -> Result<Option<TypeText>> {
    let element = |l: Option<&Literal>| match l {
        Some(l) => literal_type_text(l, type_names),
        None => Ok(None),
    };
    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    let body = match literal.literal_type.as_ref() {
        Some(LiteralType::Boolean(_)) => "boolean".to_string(),
        Some(LiteralType::I8(_)) => "i8".to_string(),
        Some(LiteralType::I16(_)) => "i16".to_string(),
        Some(LiteralType::I32(_)) => "i32".to_string(),
        Some(LiteralType::I64(_)) => "i64".to_string(),
        Some(LiteralType::Fp32(_)) => "fp32".to_string(),
        Some(LiteralType::Fp64(_)) => "fp64".to_string(),
        Some(LiteralType::String(_)) => "string".to_string(),
        Some(LiteralType::Binary(_)) => "binary".to_string(),
        Some(LiteralType::Timestamp(_)) => "timestamp".to_string(),
        Some(LiteralType::Date(_)) => "date".to_string(),
        Some(LiteralType::Time(_)) => "time".to_string(),
        Some(LiteralType::IntervalYearToMonth(_)) => "interval_year".to_string(),
        Some(LiteralType::IntervalDayToSecond(_)) => "interval_day".to_string(),
        Some(LiteralType::IntervalCompound(_)) => "interval_compound".to_string(),
        Some(LiteralType::TimestampTz(_)) => "timestamp_tz".to_string(),
        Some(LiteralType::Uuid(_)) => "uuid".to_string(),
        Some(LiteralType::FixedChar(s)) => format!("char<{}>", s.encode_utf16().count()),
        Some(LiteralType::VarChar(v)) => format!("varchar<{}>", v.length),
        Some(LiteralType::FixedBinary(b)) => format!("fixedbinary<{}>", b.len()),
        Some(LiteralType::Decimal(d)) => format!("decimal<{},{}>", d.precision, d.scale),
        Some(LiteralType::PrecisionTime(p)) => format!("precision_time<{}>", p.precision),
        Some(LiteralType::PrecisionTimestamp(p)) => {
            format!("precision_timestamp<{}>", p.precision)
        }
        Some(LiteralType::PrecisionTimestampTz(p)) => {
            format!("precision_timestamp_tz<{}>", p.precision)
        }
        Some(LiteralType::Struct(s)) => {
            let mut fields = Vec::with_capacity(s.fields.len());
            for f in &s.fields {
                let Some(t) = literal_type_text(f, type_names)? else {
                    return Ok(None);
                };
                fields.push(t);
            }
            format!("struct<{}>", joined(&fields))
        }
        Some(LiteralType::List(l)) => match element(l.values.first())? {
            Some(t) => format!("list<{t}>"),
            None => return Ok(None),
        },
        Some(LiteralType::Map(m)) => {
            let entry = m.key_values.first();
            let key = element(entry.and_then(|e| e.key.as_ref()))?;
            let value = element(entry.and_then(|e| e.value.as_ref()))?;
            match (key, value) {
                (Some(k), Some(v)) => format!("map<{k},{v}>"),
                _ => return Ok(None),
            }
        }
        Some(LiteralType::EmptyList(l)) => match l.r#type.as_deref() {
            Some(t) => format!("list<{}>", proto_type_text(t, type_names)?),
            None => return Ok(None),
        },
        Some(LiteralType::EmptyMap(m)) => match (m.key.as_deref(), m.value.as_deref()) {
            (Some(k), Some(v)) => format!(
                "map<{},{}>",
                proto_type_text(k, type_names)?,
                proto_type_text(v, type_names)?
            ),
            _ => return Ok(None),
        },
        Some(LiteralType::UserDefined(u)) => user_defined(type_names, u.type_reference),
        Some(LiteralType::Null(t)) => return proto_type_text(t, type_names).map(Some),
        None => return Ok(None),
    };
    Ok(Some(TypeText::new(body, literal.nullable)))
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
