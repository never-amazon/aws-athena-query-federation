use std::fmt;

use super::extensions::Extensions;
use super::type_converter::type_nullable;
use super::type_text::TypeText;
use crate::error::{Result, SubstraitError};
use crate::proto::expression::literal::interval_day_to_second::PrecisionMode;
use crate::proto::expression::literal::LiteralType;
use crate::proto::expression::Literal;
use crate::proto::r#type::Kind;
use crate::proto::Type;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TypeKind {
    Bool,
    I8,
    I16,
    I32,
    I64,
    Fp32,
    Fp64,
    Str,
    Binary,
    Timestamp,
    TimestampTz,
    Date,
    Time,
    IntervalYear,
    Uuid,
    IntervalDay {
        precision: i32,
    },
    IntervalCompound {
        precision: i32,
    },
    FixedChar {
        length: i32,
    },
    VarChar {
        length: i32,
    },
    FixedBinary {
        length: i32,
    },
    Decimal {
        precision: i32,
        scale: i32,
    },
    PrecisionTime {
        precision: i32,
    },
    PrecisionTimestamp {
        precision: i32,
    },
    PrecisionTimestampTz {
        precision: i32,
    },
    Struct(Vec<SubstraitType>),
    List(Box<SubstraitType>),
    Map {
        key: Box<SubstraitType>,
        value: Box<SubstraitType>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SubstraitType {
    pub(crate) kind: TypeKind,
    pub(crate) nullable: bool,
}

fn type_not_set() -> SubstraitError {
    SubstraitError::Unsupported("Type is not set: ".into())
}

fn utf16_length(value: &str) -> i32 {
    i32::try_from(value.encode_utf16().count()).unwrap_or(i32::MAX)
}

#[allow(
    deprecated,
    reason = "reads deprecated Substrait proto fields that the Java converter still reads"
)]
fn scalar_literal_kind(literal_type: &LiteralType) -> Option<TypeKind> {
    let kind = match literal_type {
        LiteralType::Boolean(_) => TypeKind::Bool,
        LiteralType::I8(_) => TypeKind::I8,
        LiteralType::I16(_) => TypeKind::I16,
        LiteralType::I32(_) => TypeKind::I32,
        LiteralType::I64(_) => TypeKind::I64,
        LiteralType::Fp32(_) => TypeKind::Fp32,
        LiteralType::Fp64(_) => TypeKind::Fp64,
        LiteralType::String(_) => TypeKind::Str,
        LiteralType::Binary(_) => TypeKind::Binary,
        LiteralType::Timestamp(_) => TypeKind::Timestamp,
        LiteralType::Date(_) => TypeKind::Date,
        LiteralType::Time(_) => TypeKind::Time,
        LiteralType::IntervalYearToMonth(_) => TypeKind::IntervalYear,
        LiteralType::IntervalDayToSecond(d) => TypeKind::IntervalDay {
            precision: match d.precision_mode {
                Some(PrecisionMode::Precision(p)) => p,
                _ => 6,
            },
        },
        LiteralType::TimestampTz(_) => TypeKind::TimestampTz,
        LiteralType::Uuid(_) => TypeKind::Uuid,
        LiteralType::FixedChar(s) => TypeKind::FixedChar {
            length: utf16_length(s),
        },
        LiteralType::VarChar(v) => TypeKind::VarChar {
            length: v.length.cast_signed(),
        },
        LiteralType::FixedBinary(b) => TypeKind::FixedBinary {
            length: i32::try_from(b.len()).unwrap_or(i32::MAX),
        },
        LiteralType::Decimal(d) => TypeKind::Decimal {
            precision: d.precision,
            scale: d.scale,
        },
        LiteralType::PrecisionTime(p) => TypeKind::PrecisionTime {
            precision: p.precision,
        },
        LiteralType::PrecisionTimestamp(p) => TypeKind::PrecisionTimestamp {
            precision: p.precision,
        },
        LiteralType::PrecisionTimestampTz(p) => TypeKind::PrecisionTimestampTz {
            precision: p.precision,
        },
        _ => return None,
    };
    Some(kind)
}

impl SubstraitType {
    pub(crate) fn new(kind: TypeKind, nullable: bool) -> SubstraitType {
        SubstraitType { kind, nullable }
    }

    pub(crate) fn boolean(nullable: bool) -> SubstraitType {
        SubstraitType::new(TypeKind::Bool, nullable)
    }

    pub(crate) fn as_nullable(&self) -> SubstraitType {
        SubstraitType {
            nullable: true,
            ..self.clone()
        }
    }

    pub(crate) fn from_proto(ty: &Type, extensions: &Extensions) -> Result<SubstraitType> {
        let nested = |t: Option<&Type>| match t {
            Some(t) => SubstraitType::from_proto(t, extensions),
            None => Err(type_not_set()),
        };
        #[allow(
            deprecated,
            reason = "reads deprecated Substrait proto fields that the Java converter still reads"
        )]
        let kind = match ty.kind.as_ref().ok_or_else(type_not_set)? {
            Kind::Bool(_) => TypeKind::Bool,
            Kind::I8(_) => TypeKind::I8,
            Kind::I16(_) => TypeKind::I16,
            Kind::I32(_) => TypeKind::I32,
            Kind::I64(_) => TypeKind::I64,
            Kind::Fp32(_) => TypeKind::Fp32,
            Kind::Fp64(_) => TypeKind::Fp64,
            Kind::String(_) => TypeKind::Str,
            Kind::Binary(_) => TypeKind::Binary,
            Kind::Timestamp(_) => TypeKind::Timestamp,
            Kind::Date(_) => TypeKind::Date,
            Kind::Time(_) => TypeKind::Time,
            Kind::IntervalYear(_) => TypeKind::IntervalYear,
            Kind::IntervalDay(d) => TypeKind::IntervalDay {
                precision: d.precision.unwrap_or(6),
            },
            Kind::IntervalCompound(c) => TypeKind::IntervalCompound {
                precision: c.precision,
            },
            Kind::TimestampTz(_) => TypeKind::TimestampTz,
            Kind::Uuid(_) => TypeKind::Uuid,
            Kind::FixedChar(c) => TypeKind::FixedChar { length: c.length },
            Kind::Varchar(c) => TypeKind::VarChar { length: c.length },
            Kind::FixedBinary(c) => TypeKind::FixedBinary { length: c.length },
            Kind::Decimal(d) => TypeKind::Decimal {
                precision: d.precision,
                scale: d.scale,
            },
            Kind::PrecisionTime(p) => TypeKind::PrecisionTime {
                precision: p.precision,
            },
            Kind::PrecisionTimestamp(p) => TypeKind::PrecisionTimestamp {
                precision: p.precision,
            },
            Kind::PrecisionTimestampTz(p) => TypeKind::PrecisionTimestampTz {
                precision: p.precision,
            },
            Kind::Struct(s) => TypeKind::Struct(
                s.types
                    .iter()
                    .map(|t| SubstraitType::from_proto(t, extensions))
                    .collect::<Result<_>>()?,
            ),
            Kind::List(l) => TypeKind::List(Box::new(nested(l.r#type.as_deref())?)),
            Kind::Map(m) => TypeKind::Map {
                key: Box::new(nested(m.key.as_deref())?),
                value: Box::new(nested(m.value.as_deref())?),
            },
            Kind::UserDefined(u) => {
                return Err(extensions.user_defined_type_error(u.type_reference))
            }
            Kind::UserDefinedTypeReference(reference) => {
                return Err(SubstraitError::Unsupported(format!(
                    "Unsupported user defined reference: user_defined_type_reference: {reference}\n"
                )))
            }
            #[allow(
                unreachable_patterns,
                reason = "keeps the Java default branch for proto kinds added later"
            )]
            _ => return Err(SubstraitError::Unsupported("Unsupported type".into())),
        };
        Ok(SubstraitType::new(kind, type_nullable(ty)))
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    pub(crate) fn of_literal(
        literal: &Literal,
        extensions: &Extensions,
    ) -> Result<Option<SubstraitType>> {
        let element = |l: Option<&Literal>| match l {
            Some(l) => SubstraitType::of_literal(l, extensions),
            None => Ok(None),
        };
        let kind = match literal.literal_type.as_ref() {
            Some(LiteralType::Struct(s)) => {
                let mut fields = Vec::with_capacity(s.fields.len());
                for f in &s.fields {
                    let Some(t) = SubstraitType::of_literal(f, extensions)? else {
                        return Ok(None);
                    };
                    fields.push(t);
                }
                TypeKind::Struct(fields)
            }
            Some(LiteralType::List(l)) => match element(l.values.first())? {
                Some(t) => TypeKind::List(Box::new(t)),
                None => return Ok(None),
            },
            Some(LiteralType::Map(m)) => {
                let entry = m.key_values.first();
                let key = element(entry.and_then(|e| e.key.as_ref()))?;
                let value = element(entry.and_then(|e| e.value.as_ref()))?;
                match (key, value) {
                    (Some(k), Some(v)) => TypeKind::Map {
                        key: Box::new(k),
                        value: Box::new(v),
                    },
                    _ => return Ok(None),
                }
            }
            Some(LiteralType::EmptyList(l)) => {
                let Some(element) = l.r#type.as_deref() else {
                    return Err(type_not_set());
                };
                TypeKind::List(Box::new(SubstraitType::from_proto(element, extensions)?))
            }
            Some(LiteralType::EmptyMap(m)) => {
                let (Some(key), Some(value)) = (m.key.as_deref(), m.value.as_deref()) else {
                    return Err(type_not_set());
                };
                TypeKind::Map {
                    key: Box::new(SubstraitType::from_proto(key, extensions)?),
                    value: Box::new(SubstraitType::from_proto(value, extensions)?),
                }
            }
            Some(LiteralType::UserDefined(u)) => {
                return Err(extensions.user_defined_type_error(u.type_reference))
            }
            Some(LiteralType::Null(t)) => {
                return SubstraitType::from_proto(t, extensions).map(Some)
            }
            Some(LiteralType::IntervalCompound(c)) => TypeKind::IntervalCompound {
                precision: match c
                    .interval_day_to_second
                    .as_ref()
                    .and_then(|d| d.precision_mode)
                {
                    Some(PrecisionMode::Precision(p)) => p,
                    _ => 6,
                },
            },
            Some(other) => match scalar_literal_kind(other) {
                Some(kind) => kind,
                None => return Ok(None),
            },
            None => return Ok(None),
        };
        Ok(Some(SubstraitType::new(kind, literal.nullable)))
    }

    pub(crate) fn signature(&self) -> TypeText {
        let joined = |types: &[SubstraitType]| {
            types
                .iter()
                .map(|t| t.signature().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let body = match &self.kind {
            TypeKind::Bool => "boolean".to_string(),
            TypeKind::I8 => "i8".to_string(),
            TypeKind::I16 => "i16".to_string(),
            TypeKind::I32 => "i32".to_string(),
            TypeKind::I64 => "i64".to_string(),
            TypeKind::Fp32 => "fp32".to_string(),
            TypeKind::Fp64 => "fp64".to_string(),
            TypeKind::Str => "string".to_string(),
            TypeKind::Binary => "binary".to_string(),
            TypeKind::Timestamp => "timestamp".to_string(),
            TypeKind::TimestampTz => "timestamp_tz".to_string(),
            TypeKind::Date => "date".to_string(),
            TypeKind::Time => "time".to_string(),
            TypeKind::IntervalYear => "interval_year".to_string(),
            TypeKind::IntervalDay { .. } => "interval_day".to_string(),
            TypeKind::IntervalCompound { .. } => "interval_compound".to_string(),
            TypeKind::Uuid => "uuid".to_string(),
            TypeKind::FixedChar { length } => format!("char<{length}>"),
            TypeKind::VarChar { length } => format!("varchar<{length}>"),
            TypeKind::FixedBinary { length } => format!("fixedbinary<{length}>"),
            TypeKind::Decimal { precision, scale } => format!("decimal<{precision},{scale}>"),
            TypeKind::PrecisionTime { precision } => format!("precision_time<{precision}>"),
            TypeKind::PrecisionTimestamp { precision } => {
                format!("precision_timestamp<{precision}>")
            }
            TypeKind::PrecisionTimestampTz { precision } => {
                format!("precision_timestamp_tz<{precision}>")
            }
            TypeKind::Struct(fields) => format!("struct<{}>", joined(fields)),
            TypeKind::List(element) => format!("list<{}>", element.signature()),
            TypeKind::Map { key, value } => {
                format!("map<{},{}>", key.signature(), value.signature())
            }
        };
        TypeText::new(body, self.nullable)
    }
}

impl fmt::Display for SubstraitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let nullable = self.nullable;
        let name = match &self.kind {
            TypeKind::Bool => "Bool",
            TypeKind::I8 => "I8",
            TypeKind::I16 => "I16",
            TypeKind::I32 => "I32",
            TypeKind::I64 => "I64",
            TypeKind::Fp32 => "FP32",
            TypeKind::Fp64 => "FP64",
            TypeKind::Str => "Str",
            TypeKind::Binary => "Binary",
            TypeKind::Timestamp => "Timestamp",
            TypeKind::TimestampTz => "TimestampTZ",
            TypeKind::Date => "Date",
            TypeKind::Time => "Time",
            TypeKind::IntervalYear => "IntervalYear",
            TypeKind::Uuid => "UUID",
            TypeKind::IntervalDay { precision } => {
                return write!(
                    f,
                    "IntervalDay{{nullable={nullable}, precision={precision}}}"
                )
            }
            TypeKind::IntervalCompound { precision } => {
                return write!(
                    f,
                    "IntervalCompound{{nullable={nullable}, precision={precision}}}"
                )
            }
            TypeKind::FixedChar { length } => {
                return write!(f, "FixedChar{{nullable={nullable}, length={length}}}")
            }
            TypeKind::VarChar { length } => {
                return write!(f, "VarChar{{nullable={nullable}, length={length}}}")
            }
            TypeKind::FixedBinary { length } => {
                return write!(f, "FixedBinary{{nullable={nullable}, length={length}}}")
            }
            TypeKind::Decimal { precision, scale } => {
                return write!(
                    f,
                    "Decimal{{nullable={nullable}, scale={scale}, precision={precision}}}"
                )
            }
            TypeKind::PrecisionTime { precision } => {
                return write!(
                    f,
                    "PrecisionTime{{nullable={nullable}, precision={precision}}}"
                )
            }
            TypeKind::PrecisionTimestamp { precision } => {
                return write!(
                    f,
                    "PrecisionTimestamp{{nullable={nullable}, precision={precision}}}"
                )
            }
            TypeKind::PrecisionTimestampTz { precision } => {
                return write!(
                    f,
                    "PrecisionTimestampTZ{{nullable={nullable}, precision={precision}}}"
                )
            }
            TypeKind::Struct(fields) => {
                write!(f, "Struct{{nullable={nullable}, fields=[")?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{field}")?;
                }
                return f.write_str("]}");
            }
            TypeKind::List(element) => {
                return write!(f, "ListType{{nullable={nullable}, elementType={element}}}")
            }
            TypeKind::Map { key, value } => {
                return write!(f, "Map{{nullable={nullable}, key={key}, value={value}}}")
            }
        };
        write!(f, "{name}{{nullable={nullable}}}")
    }
}
