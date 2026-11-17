use crate::calcite::rel_data_type::{RelDataType, RelDataTypeSystem, PRECISION_NOT_SPECIFIED};
use crate::calcite::sql_type_name::SqlTypeName;
use crate::error::{Result, SubstraitError};
use crate::proto::r#type::{Kind, Nullability};
use crate::proto::Type;

pub(crate) fn is_nullable(nullability: i32) -> bool {
    nullability == Nullability::Nullable as i32
}

fn t(system: RelDataTypeSystem, nullable: bool, name: SqlTypeName) -> RelDataType {
    RelDataType::create_sql_type(system, name).with_nullability(nullable)
}

fn tp(system: RelDataTypeSystem, nullable: bool, name: SqlTypeName, precision: i32) -> RelDataType {
    RelDataType::create_sql_type_with_precision(system, name, precision).with_nullability(nullable)
}

fn check_precision(
    system: RelDataTypeSystem,
    name: SqlTypeName,
    label: &str,
    precision: i32,
) -> Result<()> {
    let max = system.max_precision(name);
    if precision > max {
        return Err(SubstraitError::unsupported(format!(
            "unsupported {label} precision {precision}, max precision in Calcite type system is set to {max}"
        )));
    }
    Ok(())
}

pub(crate) fn kind_of(ty: &Type) -> Result<&Kind> {
    ty.kind
        .as_ref()
        .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))
}

pub(crate) fn type_nullable(ty: &Type) -> bool {
    #[allow(deprecated)]
    let n = match &ty.kind {
        Some(Kind::Bool(x)) => x.nullability,
        Some(Kind::I8(x)) => x.nullability,
        Some(Kind::I16(x)) => x.nullability,
        Some(Kind::I32(x)) => x.nullability,
        Some(Kind::I64(x)) => x.nullability,
        Some(Kind::Fp32(x)) => x.nullability,
        Some(Kind::Fp64(x)) => x.nullability,
        Some(Kind::String(x)) => x.nullability,
        Some(Kind::Binary(x)) => x.nullability,
        Some(Kind::Timestamp(x)) => x.nullability,
        Some(Kind::Date(x)) => x.nullability,
        Some(Kind::Time(x)) => x.nullability,
        Some(Kind::IntervalYear(x)) => x.nullability,
        Some(Kind::IntervalDay(x)) => x.nullability,
        Some(Kind::IntervalCompound(x)) => x.nullability,
        Some(Kind::TimestampTz(x)) => x.nullability,
        Some(Kind::Uuid(x)) => x.nullability,
        Some(Kind::FixedChar(x)) => x.nullability,
        Some(Kind::Varchar(x)) => x.nullability,
        Some(Kind::FixedBinary(x)) => x.nullability,
        Some(Kind::Decimal(x)) => x.nullability,
        Some(Kind::PrecisionTime(x)) => x.nullability,
        Some(Kind::PrecisionTimestamp(x)) => x.nullability,
        Some(Kind::PrecisionTimestampTz(x)) => x.nullability,
        Some(Kind::Struct(x)) => x.nullability,
        Some(Kind::List(x)) => x.nullability,
        Some(Kind::Map(x)) => x.nullability,
        Some(Kind::UserDefined(x)) => x.nullability,
        _ => 0,
    };
    is_nullable(n)
}

pub(crate) fn type_string(ty: &Type) -> &'static str {
    #[allow(deprecated)]
    match &ty.kind {
        Some(Kind::Bool(_)) => "bool",
        Some(Kind::I8(_)) => "i8",
        Some(Kind::I16(_)) => "i16",
        Some(Kind::I32(_)) => "i32",
        Some(Kind::I64(_)) => "i64",
        Some(Kind::Fp32(_)) => "fp32",
        Some(Kind::Fp64(_)) => "fp64",
        Some(Kind::String(_)) => "str",
        Some(Kind::Binary(_)) => "vbin",
        Some(Kind::Timestamp(_)) => "ts",
        Some(Kind::Date(_)) => "date",
        Some(Kind::Time(_)) => "time",
        Some(Kind::IntervalYear(_)) => "iyear",
        Some(Kind::IntervalDay(_)) => "iday",
        Some(Kind::IntervalCompound(_)) => "icompound",
        Some(Kind::TimestampTz(_)) => "tstz",
        Some(Kind::Uuid(_)) => "uuid",
        Some(Kind::FixedChar(_)) => "fchar",
        Some(Kind::Varchar(_)) => "vchar",
        Some(Kind::FixedBinary(_)) => "fbin",
        Some(Kind::Decimal(_)) => "dec",
        Some(Kind::PrecisionTime(_)) => "pt",
        Some(Kind::PrecisionTimestamp(_)) => "pts",
        Some(Kind::PrecisionTimestampTz(_)) => "ptstz",
        Some(Kind::Struct(_)) => "struct",
        Some(Kind::List(_)) => "list",
        Some(Kind::Map(_)) => "map",
        Some(Kind::UserDefined(_)) => "u!",
        _ => "",
    }
}

pub(crate) fn to_calcite(system: RelDataTypeSystem, ty: &Type) -> Result<RelDataType> {
    let n = type_nullable(ty);
    #[allow(deprecated)]
    let out = match kind_of(ty)? {
        Kind::Bool(_) => t(system, n, SqlTypeName::Boolean),
        Kind::I8(_) => t(system, n, SqlTypeName::Tinyint),
        Kind::I16(_) => t(system, n, SqlTypeName::Smallint),
        Kind::I32(_) => t(system, n, SqlTypeName::Integer),
        Kind::I64(_) => t(system, n, SqlTypeName::Bigint),
        Kind::Fp32(_) => t(system, n, SqlTypeName::Real),
        Kind::Fp64(_) => t(system, n, SqlTypeName::Double),
        Kind::String(_) => t(system, n, SqlTypeName::Varchar),
        Kind::Binary(_) => t(system, n, SqlTypeName::Varbinary),
        Kind::Date(_) => t(system, n, SqlTypeName::Date),
        Kind::Time(_) => tp(system, n, SqlTypeName::Time, 6),
        Kind::TimestampTz(_) => tp(system, n, SqlTypeName::TimestampWithLocalTimeZone, 6),
        Kind::Timestamp(_) => tp(system, n, SqlTypeName::Timestamp, 6),
        Kind::PrecisionTime(p) => {
            check_precision(system, SqlTypeName::Time, "precision_time", p.precision)?;
            tp(system, n, SqlTypeName::Time, p.precision)
        }
        Kind::PrecisionTimestamp(p) => {
            check_precision(
                system,
                SqlTypeName::Timestamp,
                "precision_timestamp",
                p.precision,
            )?;
            tp(system, n, SqlTypeName::Timestamp, p.precision)
        }
        Kind::PrecisionTimestampTz(p) => {
            check_precision(
                system,
                SqlTypeName::TimestampWithLocalTimeZone,
                "precision_timestamp_tz",
                p.precision,
            )?;
            tp(
                system,
                n,
                SqlTypeName::TimestampWithLocalTimeZone,
                p.precision,
            )
        }
        Kind::IntervalYear(_) => {
            RelDataType::interval(SqlTypeName::IntervalYearMonth, PRECISION_NOT_SPECIFIED)
                .with_nullability(n)
        }
        Kind::IntervalDay(d) => {
            RelDataType::interval(SqlTypeName::IntervalDaySecond, d.precision.unwrap_or(6))
                .with_nullability(n)
        }
        Kind::FixedChar(c) => tp(system, n, SqlTypeName::Char, c.length),
        Kind::Varchar(c) => tp(system, n, SqlTypeName::Varchar, c.length),
        Kind::FixedBinary(c) => tp(system, n, SqlTypeName::Binary, c.length),
        Kind::Decimal(d) => RelDataType::create_sql_type_with_scale(
            system,
            SqlTypeName::Decimal,
            d.precision,
            d.scale,
        )
        .with_nullability(n),
        Kind::Struct(s) => {
            let mut fields = Vec::with_capacity(s.types.len());
            for (i, f) in s.types.iter().enumerate() {
                fields.push((format!("f{i}"), to_calcite(system, f)?));
            }
            RelDataType::row(fields, n)
        }
        Kind::List(l) => {
            let element = l
                .r#type
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            RelDataType::array(to_calcite(system, element)?, n)
        }
        Kind::Map(m) => {
            let key = m
                .key
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            let value = m
                .value
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            RelDataType::map(to_calcite(system, key)?, to_calcite(system, value)?, n)
        }
        Kind::UserDefined(u) => {
            return Err(SubstraitError::unsupported(format!(
                "Unable to map user-defined type: {}",
                u.type_reference
            )))
        }
        Kind::IntervalCompound(_) | Kind::Uuid(_) => {
            return Err(SubstraitError::runtime("Unknown expression type."))
        }
        Kind::UserDefinedTypeReference(_) => {
            return Err(SubstraitError::unsupported(
                "Unsupported type: USER_DEFINED_TYPE_REFERENCE",
            ))
        }
        #[allow(unreachable_patterns)]
        _ => return Err(SubstraitError::unsupported("Unsupported type")),
    };
    Ok(out)
}

pub(crate) fn named_struct_to_calcite(
    system: RelDataTypeSystem,
    names: &[String],
    types: &[Type],
    nullable: bool,
) -> Result<RelDataType> {
    let mut position = 0usize;
    let mut fields = Vec::with_capacity(types.len());
    for ty in types {
        let name = names.get(position).cloned().ok_or_else(|| {
            SubstraitError::index_out_of_bounds(format!(
                "Index {position} out of bounds for length {}",
                names.len()
            ))
        })?;
        position += 1;
        let (converted, next) = to_calcite_named(system, ty, names, position)?;
        position = next;
        fields.push((name, converted));
    }
    Ok(RelDataType::row(fields, nullable))
}

fn to_calcite_named(
    system: RelDataTypeSystem,
    ty: &Type,
    names: &[String],
    position: usize,
) -> Result<(RelDataType, usize)> {
    match kind_of(ty)? {
        Kind::Struct(s) => {
            let mut pos = position;
            let mut fields = Vec::with_capacity(s.types.len());
            for f in &s.types {
                let name = names.get(pos).cloned().ok_or_else(|| {
                    SubstraitError::index_out_of_bounds(format!(
                        "Index {pos} out of bounds for length {}",
                        names.len()
                    ))
                })?;
                pos += 1;
                let (converted, next) = to_calcite_named(system, f, names, pos)?;
                pos = next;
                fields.push((name, converted));
            }
            Ok((RelDataType::row(fields, type_nullable(ty)), pos))
        }
        Kind::List(l) => {
            let element = l
                .r#type
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            let (e, next) = to_calcite_named(system, element, names, position)?;
            Ok((RelDataType::array(e, type_nullable(ty)), next))
        }
        Kind::Map(m) => {
            let key = m
                .key
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            let value = m
                .value
                .as_deref()
                .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
            let (k, next) = to_calcite_named(system, key, names, position)?;
            let (v, next) = to_calcite_named(system, value, names, next)?;
            Ok((RelDataType::map(k, v, type_nullable(ty)), next))
        }
        _ => Ok((to_calcite(system, ty)?, position)),
    }
}
