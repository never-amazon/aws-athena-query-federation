use crate::java_cast::{
    double_to_float, int_to_float, long_to_byte, long_to_char, long_to_float, long_to_int,
    long_to_short,
};
use std::str::FromStr;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use bigdecimal::{BigDecimal, RoundingMode};
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};

use super::block::Block;
use super::block_allocator::BlockAllocator;
use super::date_time_formatter_util::DateTimeFormatterUtil;
use super::field_resolver::FieldResolver;
use super::java_format::{big_decimal_to_string, bytes_to_hex, double_to_string, float_to_string};
use super::java_time::{
    local_date_of_epoch_day, local_date_to_epoch_day, local_date_to_string, ZoneId,
};
use super::schema_builder::SchemaBuilder;
use super::types::{
    arrow_type_to_string, decimal_precision_scale, get_children, minor_type_of, timestamp_timezone,
    ArrowType, MinorType,
};
use super::value::Value;
use super::vector::{FieldReader, FieldVector};
use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

pub struct BlockUtils;

pub const UTC_ZONE_ID: &str = "UTC";

pub const MAP_DATA_VECTOR_NAME: &str = "entries";
pub const MAP_KEY_NAME: &str = "key";
pub const MAP_VALUE_NAME: &str = "value";

fn class_cast(value: &Value, target: &str) -> FederationError {
    FederationError::ClassCast(format!(
        "class {} cannot be cast to class {}",
        value.java_class_name(),
        target
    ))
}

fn is_boxed_integer(value: &Value) -> bool {
    matches!(
        value,
        Value::Long(_) | Value::Int(_) | Value::Short(_) | Value::Byte(_)
    )
}

pub fn to_big_decimal(value: &Value) -> Result<BigDecimal> {
    match value {
        Value::Decimal(d) => Ok(d.clone()),
        Value::Double(d) => BigDecimal::from_str(&double_to_string(*d)).map_err(|_| {
            FederationError::Runtime(format!(
                "Character array is missing \"exponent\" mark 'e' or 'E': {d}"
            ))
        }),
        Value::Float(f) => BigDecimal::from_str(&double_to_string(f64::from(*f)))
            .map_err(|_| FederationError::Runtime(format!("invalid number {f}"))),
        Value::Byte(v) => Ok(BigDecimal::from(i64::from(*v))),
        Value::Short(v) => Ok(BigDecimal::from(i64::from(*v))),
        Value::Int(v) => Ok(BigDecimal::from(i64::from(*v))),
        Value::Long(v) => Ok(BigDecimal::from(*v)),
        Value::BigInteger(v) => Ok(BigDecimal::new(v.clone(), 0)),
        other => Err(class_cast(other, "java.lang.Number")),
    }
}

fn exact_integer(value: &BigDecimal) -> Result<BigInt> {
    if !value.is_integer() {
        return Err(FederationError::Arithmetic("Rounding necessary".into()));
    }
    let (int_part, _) = value.with_scale(0).into_bigint_and_exponent();
    Ok(int_part)
}

pub fn to_exact_long(value: &Value) -> Result<i64> {
    if is_boxed_integer(value) {
        return value
            .number_long_value()
            .ok_or_else(|| class_cast(value, "java.lang.Number"));
    }
    exact_integer(&to_big_decimal(value)?)?
        .to_i64()
        .ok_or_else(|| FederationError::Arithmetic("Overflow".into()))
}

pub fn to_exact_int(value: &Value) -> Result<i32> {
    if is_boxed_integer(value) {
        return value
            .number_int_value()
            .ok_or_else(|| class_cast(value, "java.lang.Number"));
    }
    exact_integer(&to_big_decimal(value)?)?
        .to_i32()
        .ok_or_else(|| FederationError::Arithmetic("Overflow".into()))
}

pub fn to_exact_short(value: &Value) -> Result<i16> {
    if is_boxed_integer(value) {
        return value
            .number_long_value()
            .map(long_to_short)
            .ok_or_else(|| class_cast(value, "java.lang.Number"));
    }
    exact_integer(&to_big_decimal(value)?)?
        .to_i16()
        .ok_or_else(|| FederationError::Arithmetic("Overflow".into()))
}

pub fn to_exact_byte(value: &Value) -> Result<i8> {
    if is_boxed_integer(value) {
        return value
            .number_long_value()
            .map(long_to_byte)
            .ok_or_else(|| class_cast(value, "java.lang.Number"));
    }
    exact_integer(&to_big_decimal(value)?)?
        .to_i8()
        .ok_or_else(|| FederationError::Arithmetic("Overflow".into()))
}

fn is_simple_minor_type(minor: MinorType) -> bool {
    matches!(
        minor,
        MinorType::TIMESTAMPMILLITZ
            | MinorType::TIMESTAMPMICROTZ
            | MinorType::DATEMILLI
            | MinorType::DATEDAY
            | MinorType::FLOAT8
            | MinorType::FLOAT4
            | MinorType::INT
            | MinorType::TINYINT
            | MinorType::SMALLINT
            | MinorType::UINT1
            | MinorType::UINT2
            | MinorType::UINT4
            | MinorType::UINT8
            | MinorType::BIGINT
            | MinorType::VARBINARY
            | MinorType::DECIMAL
            | MinorType::VARCHAR
            | MinorType::BIT
    )
}

fn unknown_simple_type(field: &Field, minor: MinorType) -> FederationError {
    FederationError::not_supported(format!(
        "Unknown type {} {}",
        arrow_type_to_string(field.data_type()),
        minor
    ))
}

fn java_date_to_days(millis: i64) -> i32 {
    let seconds = millis.div_euclid(1000);
    long_to_int(seconds / 86400)
}

fn number_double_value(value: &Value) -> Result<f64> {
    if !value.is_number() {
        return Err(class_cast(value, "java.lang.Number"));
    }
    value
        .as_f64()
        .ok_or_else(|| class_cast(value, "java.lang.Number"))
}

fn number_float_value(value: &Value) -> Result<f32> {
    match value {
        Value::Float(f) => Ok(*f),
        Value::Long(v) => Ok(long_to_float(*v)),
        Value::Int(v) => Ok(int_to_float(*v)),
        other => number_double_value(other).map(double_to_float),
    }
}

fn set_scale_half_up(value: &BigDecimal, scale: i8) -> BigDecimal {
    value.with_scale_round(i64::from(scale), RoundingMode::HalfUp)
}

impl BlockUtils {
    pub fn new_block<I, V>(
        allocator: &dyn BlockAllocator,
        column_name: &str,
        arrow_type: &ArrowType,
        values: I,
    ) -> Result<Block>
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        let schema = SchemaBuilder::new()
            .add_field_with_type(column_name, arrow_type.clone())
            .build();
        let mut block = allocator.create_block(Arc::new(schema));
        let mut count = 0usize;
        for next in values {
            let next: Value = next.into();
            let vector = block.get_field_vector_mut(column_name).ok_or_else(|| {
                FederationError::NullPointer(format!("missing vector {column_name}"))
            })?;
            if let Err(ex) = BlockUtils::set_value(vector, count, &next) {
                return Err(FederationError::connector_with_details(
                    FederationSourceErrorCode::InvalidInputException,
                    format!(
                        "Error for {} {} {}",
                        arrow_type_to_string(arrow_type),
                        column_name,
                        next
                    ),
                    ex.to_string(),
                ));
            }
            count += 1;
        }
        block.set_row_count(count);
        Ok(block)
    }

    pub fn new_empty_block(
        allocator: &dyn BlockAllocator,
        column_name: &str,
        arrow_type: &ArrowType,
    ) -> Block {
        let schema: Schema = SchemaBuilder::new()
            .add_field_with_type(column_name, arrow_type.clone())
            .build();
        allocator.create_block(Arc::new(schema))
    }

    pub fn set_complex_value(
        vector: &mut FieldVector,
        pos: usize,
        resolver: &dyn FieldResolver,
        value: &Value,
    ) -> Result<()> {
        let field = vector.field().clone();
        match vector.minor_type() {
            MinorType::MAP => BlockUtils::write_map(vector, &field, pos, value, resolver),
            MinorType::LIST => {
                BlockUtils::write_list(vector, &field, pos, value, resolver).map(|_| ())
            }
            MinorType::STRUCT => BlockUtils::write_struct(vector, &field, pos, value, resolver),
            other => Err(FederationError::not_supported(format!(
                "Unsupported 'Complex' vector {} for field {}",
                other,
                field.name()
            ))),
        }
    }

    pub fn set_value(vector: &mut FieldVector, pos: usize, value: &Value) -> Result<()> {
        match BlockUtils::set_value_inner(vector, pos, value) {
            Ok(()) => Ok(()),
            Err(_) => Err(FederationError::invalid_input(format!(
                "Unable to set value for field {} using value {} of type {}",
                vector.name(),
                value,
                vector.minor_type()
            ))),
        }
    }

    fn set_value_inner(vector: &mut FieldVector, pos: usize, value: &Value) -> Result<()> {
        if value.is_null() {
            return BlockUtils::set_null_value(vector, pos);
        }
        let minor = vector.minor_type();
        let field = vector.field();
        let canonical = match minor {
            MinorType::TIMESTAMPMILLITZ | MinorType::TIMESTAMPMICROTZ => {
                if let Value::Long(v) = value {
                    Value::Long(*v)
                } else {
                    let tz = timestamp_timezone(field.data_type());
                    Value::Long(if minor == MinorType::TIMESTAMPMILLITZ {
                        DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(value, tz)?
                            .value
                    } else {
                        DateTimeFormatterUtil::timestamp_micro_tz_holder_from_object(value, tz)?
                            .value
                    })
                }
            }
            MinorType::DATEMILLI => {
                let ms = match value {
                    Value::Date(ms) => *ms,
                    Value::LocalDateTime(ldt) => {
                        ZoneId::utc().instant_of_local(ldt).timestamp_millis()
                    }
                    Value::Instant(i) => i.timestamp_millis(),
                    Value::Long(v) => *v,
                    other => return Err(class_cast(other, "java.lang.Long")),
                };
                Value::Long(ms)
            }
            MinorType::DATEDAY => {
                let days = match value {
                    Value::Date(ms) => java_date_to_days(*ms),
                    Value::LocalDate(d) => long_to_int(local_date_to_epoch_day(d)),
                    Value::Long(v) => long_to_int(*v),
                    Value::Int(v) => *v,
                    other => return Err(class_cast(other, "java.lang.Integer")),
                };
                Value::Int(days)
            }
            MinorType::FLOAT8 => Value::Double(number_double_value(value)?),
            MinorType::FLOAT4 => Value::Float(number_float_value(value)?),
            MinorType::INT => Value::Int(to_exact_int(value)?),
            MinorType::TINYINT => match value {
                Value::Byte(b) => Value::Byte(*b),
                other => Value::Byte(to_exact_byte(other)?),
            },
            MinorType::SMALLINT => match value {
                Value::Short(s) => Value::Short(*s),
                other => Value::Short(to_exact_short(other)?),
            },
            MinorType::UINT1 => match value {
                Value::Byte(b) => Value::Byte(*b),
                Value::Int(i) => Value::Byte(long_to_byte(i64::from(*i))),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::UINT2 => match value {
                Value::Char(c) => Value::Char(*c),
                Value::Int(i) => Value::Char(long_to_char(i64::from(*i))),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::UINT4 => match value {
                Value::Int(i) => Value::Int(*i),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::UINT8 => match value {
                Value::Long(l) => Value::Long(*l),
                Value::Int(i) => Value::Long(i64::from(*i)),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::BIGINT => Value::Long(to_exact_long(value)?),
            MinorType::VARBINARY => match value {
                Value::Bytes(b) => Value::Bytes(b.clone()),
                other => return Err(class_cast(other, "[B")),
            },
            MinorType::DECIMAL => {
                let (_, scale) = decimal_precision_scale(field.data_type()).unwrap_or((38, 0));
                let d = set_scale_half_up(&to_big_decimal(value)?, scale);
                Value::Decimal(d)
            }
            MinorType::VARCHAR => match value {
                Value::String(s) => Value::String(s.clone()),
                other => Value::String(other.java_to_string()),
            },
            MinorType::BIT => {
                let bit = matches!(value, Value::Int(i) if *i > 0)
                    || matches!(value, Value::Boolean(true));
                Value::Boolean(bit)
            }
            other => {
                return Err(FederationError::invalid_input(format!(
                    "Unknown type {other}"
                )))
            }
        };
        vector.set(pos, &canonical)
    }

    fn set_null_value(vector: &mut FieldVector, pos: usize) -> Result<()> {
        match vector.minor_type() {
            MinorType::TIMESTAMPMICROTZ
            | MinorType::TIMESTAMPMILLITZ
            | MinorType::DATEMILLI
            | MinorType::DATEDAY
            | MinorType::FLOAT8
            | MinorType::FLOAT4
            | MinorType::INT
            | MinorType::TINYINT
            | MinorType::SMALLINT
            | MinorType::UINT1
            | MinorType::UINT2
            | MinorType::UINT4
            | MinorType::UINT8
            | MinorType::BIGINT
            | MinorType::VARBINARY
            | MinorType::DECIMAL
            | MinorType::VARCHAR
            | MinorType::BIT => {
                vector.set_null(pos);
                Ok(())
            }
            other => Err(FederationError::invalid_input(format!(
                "Unknown type {other}"
            ))),
        }
    }

    pub fn row_to_string(block: &Block, row: usize) -> Result<String> {
        if row > block.row_count() {
            return Err(FederationError::invalid_input(format!(
                "{} exceeds available rows {}",
                row,
                block.row_count()
            )));
        }
        let mut sb = String::new();
        for reader in block.field_readers() {
            reader.set_position(row);
            if !sb.is_empty() {
                sb.push_str(", ");
            }
            sb.push('[');
            sb.push_str(reader.field().name());
            sb.push_str(" : ");
            match BlockUtils::field_to_string(&reader) {
                Ok(s) => sb.push_str(&s),
                Err(ex) => {
                    return Err(FederationError::connector_with_details(
                        FederationSourceErrorCode::InternalServiceException,
                        format!("Error processing field {}", reader.field().name()),
                        ex.to_string(),
                    ))
                }
            }
            sb.push(']');
        }
        Ok(sb)
    }

    pub fn field_to_string(reader: &FieldReader<'_>) -> Result<String> {
        let npe = || FederationError::NullPointer("null value".into());
        let minor = reader.minor_type();
        Ok(match minor {
            MinorType::DATEDAY => {
                let days = reader.read_integer().ok_or_else(npe)?;
                let d = local_date_of_epoch_day(i64::from(days)).ok_or_else(npe)?;
                local_date_to_string(&d)
            }
            MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ => {
                let v = reader.read_long().ok_or_else(npe)?;
                DateTimeFormatterUtil::construct_zoned_date_time(v, reader.field().data_type())?
                    .to_string()
            }
            MinorType::DATEMILLI
            | MinorType::FLOAT8
            | MinorType::FLOAT4
            | MinorType::UINT4
            | MinorType::UINT8
            | MinorType::INT
            | MinorType::BIGINT
            | MinorType::VARCHAR
            | MinorType::BIT => reader.read_object().java_to_string(),
            MinorType::DECIMAL => match reader.read_big_decimal() {
                Some(d) => big_decimal_to_string(&d),
                None => "null".to_string(),
            },
            MinorType::SMALLINT => match reader.read_short() {
                Some(s) => s.to_string(),
                None => "null".to_string(),
            },
            MinorType::TINYINT | MinorType::UINT1 => {
                reader.read_byte().ok_or_else(npe)?.to_string()
            }
            MinorType::UINT2 => reader.read_character().ok_or_else(npe)?.to_string(),
            MinorType::VARBINARY => bytes_to_hex(&reader.read_byte_array().ok_or_else(npe)?),
            MinorType::STRUCT => {
                let mut sb = String::from("{");
                for child in get_children(reader.field()) {
                    if sb.len() > 3 {
                        sb.push(',');
                    }
                    sb.push('[');
                    sb.push_str(child.name());
                    sb.push_str(" : ");
                    let child_reader = reader.reader_named(child.name()).ok_or_else(npe)?;
                    sb.push_str(&BlockUtils::field_to_string(&child_reader)?);
                    sb.push(']');
                }
                sb.push('}');
                sb
            }
            MinorType::LIST => {
                let mut sb = String::from("{");
                while reader.next() {
                    if sb.len() > 1 {
                        sb.push(',');
                    }
                    let child = reader.reader().ok_or_else(npe)?;
                    sb.push_str(&BlockUtils::field_to_string(&child)?);
                }
                sb.push('}');
                sb
            }
            MinorType::MAP => {
                let mut sb = String::new();
                while reader.next() {
                    let child = reader.reader().ok_or_else(npe)?;
                    sb.push_str(&BlockUtils::field_to_string(&child)?);
                }
                sb
            }
            other => {
                let obj = reader.read_object();
                let class = if obj.is_null() {
                    "null".to_string()
                } else {
                    format!("class {}", obj.java_class_name())
                };
                format!("{other} - {class}[ {obj} ]")
            }
        })
    }

    pub fn copy_rows(
        src_block: &Block,
        dst_block: &mut Block,
        first_row: usize,
        last_row: usize,
    ) -> Result<usize> {
        if first_row > last_row || last_row >= src_block.row_count() {
            return Err(FederationError::internal(format!(
                "src has {} but requested copy of {} to {}",
                src_block.row_count(),
                first_row,
                last_row
            )));
        }
        for src in src_block.field_readers() {
            let start = dst_block.row_count();
            for (dst_offset, i) in (start..).zip(first_row..=last_row) {
                let name = src.field().name().clone();
                let dst = dst_block.get_field_vector_mut(&name).ok_or_else(|| {
                    FederationError::NullPointer(format!("No vector found for field {name}"))
                })?;
                src.set_position(i);
                BlockUtils::set_value(dst, dst_offset, &src.read_object())?;
            }
        }
        let rows_copied = 1 + (last_row - first_row);
        let new_count = dst_block.row_count() + rows_copied;
        dst_block.set_row_count(new_count);
        Ok(rows_copied)
    }

    pub fn is_null_row(block: &Block, row: usize) -> Result<bool> {
        if row >= block.row_count() {
            return Err(FederationError::invalid_input(format!(
                "block has {} rows but requested to check {}",
                block.row_count(),
                row
            )));
        }
        for src in block.field_readers() {
            src.set_position(row);
            if src.is_set() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn write_list(
        vector: &mut FieldVector,
        field: &Field,
        pos: usize,
        value: &Value,
        resolver: &dyn FieldResolver,
    ) -> Result<bool> {
        if value.is_null() {
            vector.set_null(pos);
            return Ok(true);
        }
        let items = match value {
            Value::List(items) => items,
            other => return Err(class_cast(other, "java.util.List")),
        };
        let child_field = get_children(field).into_iter().next();
        let start = vector.start_list(pos)?;
        let mut end = start;
        if let Some(child_field) = child_field {
            let child_vector = vector
                .data_vector_mut()
                .ok_or_else(|| FederationError::illegal_state("list vector has no data vector"))?;
            for item in items {
                if BlockUtils::write_all_value(
                    child_vector,
                    &child_field,
                    end,
                    resolver,
                    item,
                    false,
                )? {
                    end += 1;
                }
            }
        }
        vector.end_list(pos, end)?;
        Ok(true)
    }

    pub fn write_struct(
        vector: &mut FieldVector,
        field: &Field,
        pos: usize,
        value: &Value,
        resolver: &dyn FieldResolver,
    ) -> Result<()> {
        if value.is_null() {
            vector.set_null(pos);
            return Ok(());
        }
        vector.set_index_defined(pos);
        for next_child in get_children(field) {
            let child_value = resolver.get_field_value(&next_child, value)?;
            let child_vector = vector.get_child_mut(next_child.name()).ok_or_else(|| {
                FederationError::illegal_state(format!(
                    "missing child vector {}",
                    next_child.name()
                ))
            })?;
            BlockUtils::write_all_value(
                child_vector,
                &next_child,
                pos,
                resolver,
                &child_value,
                true,
            )?;
        }
        Ok(())
    }

    fn invalid_map_schema(field: &Field) -> FederationError {
        FederationError::invalid_input(format!("Invalid Arrow Map schema: {field:?}"))
    }

    pub fn write_map(
        vector: &mut FieldVector,
        field: &Field,
        pos: usize,
        value: &Value,
        resolver: &dyn FieldResolver,
    ) -> Result<()> {
        if value.is_null() {
            vector.set_null(pos);
            return Ok(());
        }
        let children = get_children(field);
        if children.len() != 1 {
            return Err(BlockUtils::invalid_map_schema(field));
        }
        let kv_field = children[0].clone();
        if kv_field.name() != MAP_DATA_VECTOR_NAME
            || !matches!(kv_field.data_type(), DataType::Struct(_))
        {
            return Err(BlockUtils::invalid_map_schema(field));
        }
        let kv_children = get_children(&kv_field);
        if kv_children.len() != 2 {
            return Err(BlockUtils::invalid_map_schema(field));
        }
        let key_field = kv_children[0].clone();
        let value_field = kv_children[1].clone();
        if key_field.name() != MAP_KEY_NAME || value_field.name() != MAP_VALUE_NAME {
            return Err(BlockUtils::invalid_map_schema(field));
        }
        let fetched;
        let map_value = if let Value::Map(_) = value {
            value
        } else {
            fetched = resolver.get_field_value(field, value)?;
            &fetched
        };
        let entries = match map_value {
            Value::Map(entries) => entries,
            other => return Err(class_cast(other, "java.util.Map")),
        };
        let start = vector.start_list(pos)?;
        let mut idx = start;
        {
            let entries_vector = vector.data_vector_mut().ok_or_else(|| {
                FederationError::illegal_state("map vector has no entries vector")
            })?;
            for (k, v) in entries {
                entries_vector.set_index_defined(idx);
                let key = if minor_type_of(key_field.data_type()) == MinorType::STRUCT {
                    k.clone()
                } else {
                    resolver.get_map_key(&key_field, k)?
                };
                let key_vector = entries_vector
                    .get_child_mut(MAP_KEY_NAME)
                    .ok_or_else(|| BlockUtils::invalid_map_schema(field))?;
                BlockUtils::write_all_value(key_vector, &key_field, idx, resolver, &key, true)?;
                if !v.is_null() {
                    let val = if minor_type_of(value_field.data_type()) == MinorType::STRUCT {
                        v.clone()
                    } else {
                        resolver.get_map_value(&value_field, v)?
                    };
                    let value_vector = entries_vector
                        .get_child_mut(MAP_VALUE_NAME)
                        .ok_or_else(|| BlockUtils::invalid_map_schema(field))?;
                    BlockUtils::write_all_value(
                        value_vector,
                        &value_field,
                        idx,
                        resolver,
                        &val,
                        true,
                    )?;
                }
                idx += 1;
            }
            entries_vector.set_value_count(idx.max(entries_vector.value_count()));
        }
        vector.end_list(pos, idx)?;
        Ok(())
    }

    pub fn write_all_value(
        vector: &mut FieldVector,
        field: &Field,
        pos: usize,
        resolver: &dyn FieldResolver,
        value: &Value,
        from_map_or_struct: bool,
    ) -> Result<bool> {
        match minor_type_of(field.data_type()) {
            MinorType::LIST => BlockUtils::write_list(vector, field, pos, value, resolver),
            MinorType::STRUCT => {
                BlockUtils::write_struct(vector, field, pos, value, resolver).map(|()| true)
            }
            MinorType::MAP => {
                BlockUtils::write_map(vector, field, pos, value, resolver).map(|()| true)
            }
            _ => BlockUtils::write_simple_value(vector, field, pos, value, from_map_or_struct),
        }
    }

    pub fn write_simple_value(
        vector: &mut FieldVector,
        field: &Field,
        pos: usize,
        value: &Value,
        _from_map_or_struct: bool,
    ) -> Result<bool> {
        let minor = minor_type_of(field.data_type());
        BlockUtils::write_simple_value_inner(vector, field, minor, pos, value)
            .map_err(|_| BlockUtils::write_error(field, minor, value))
    }

    fn write_error(field: &Field, minor: MinorType, value: &Value) -> FederationError {
        FederationError::invalid_input(format!(
            "Unable to write value for field {} using value {} with minor type {}",
            field.name(),
            value,
            minor
        ))
    }

    fn write_simple_value_inner(
        vector: &mut FieldVector,
        field: &Field,
        minor: MinorType,
        pos: usize,
        value: &Value,
    ) -> Result<bool> {
        if value.is_null() {
            if !is_simple_minor_type(minor) {
                return Err(unknown_simple_type(field, minor));
            }
            vector.set_null(pos);
            return Ok(true);
        }
        match BlockUtils::simple_stored_value(field, minor, value)? {
            Some(stored) => {
                vector.set(pos, &stored)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn simple_stored_value(
        field: &Field,
        minor: MinorType,
        value: &Value,
    ) -> Result<Option<Value>> {
        Ok(Some(match minor {
            MinorType::TIMESTAMPMILLITZ => {
                let tz = timestamp_timezone(field.data_type());
                Value::Long(
                    DateTimeFormatterUtil::timestamp_milli_tz_holder_from_object(value, tz)?.value,
                )
            }
            MinorType::TIMESTAMPMICROTZ => {
                let tz = timestamp_timezone(field.data_type());
                Value::Long(
                    DateTimeFormatterUtil::timestamp_micro_tz_holder_from_object(value, tz)?.value,
                )
            }
            MinorType::DATEMILLI => match value {
                Value::Date(ms) => Value::Long(*ms),
                Value::LocalDateTime(ldt) => {
                    Value::Long(ZoneId::utc().instant_of_local(ldt).timestamp_millis())
                }
                Value::Long(v) => Value::Long(*v),
                other => return Err(class_cast(other, "java.lang.Long")),
            },
            MinorType::DATEDAY => match value {
                Value::Date(ms) => Value::Int(java_date_to_days(*ms)),
                Value::LocalDate(d) => Value::Int(long_to_int(local_date_to_epoch_day(d))),
                Value::Long(v) => Value::Int(long_to_int(*v)),
                Value::Int(v) => Value::Int(*v),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::FLOAT8 => match value {
                Value::Int(i) => Value::Double(f64::from(*i)),
                Value::Double(d) => Value::Double(*d),
                other => return Err(class_cast(other, "java.lang.Double")),
            },
            MinorType::FLOAT4 => match value {
                Value::Float(f) => Value::Float(*f),
                other => return Err(class_cast(other, "java.lang.Float")),
            },
            MinorType::INT => match value {
                Value::Long(l) => Value::Int(long_to_int(*l)),
                Value::Int(i) => Value::Int(*i),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::TINYINT | MinorType::UINT1 => match value {
                Value::Byte(b) => Value::Byte(*b),
                other => return Err(class_cast(other, "java.lang.Byte")),
            },
            MinorType::SMALLINT => match value {
                Value::Short(s) => Value::Short(*s),
                other => return Err(class_cast(other, "java.lang.Short")),
            },
            MinorType::UINT2 => match value {
                Value::Char(c) => Value::Char(*c),
                other => return Err(class_cast(other, "java.lang.Character")),
            },
            MinorType::UINT4 => match value {
                Value::Int(i) => Value::Int(*i),
                other => return Err(class_cast(other, "java.lang.Integer")),
            },
            MinorType::UINT8 | MinorType::BIGINT => match value {
                Value::Long(l) => Value::Long(*l),
                other => return Err(class_cast(other, "java.lang.Long")),
            },
            MinorType::VARBINARY => match value {
                Value::Bytes(b) => Value::Bytes(b.clone()),
                _ => return Ok(None),
            },
            MinorType::DECIMAL => {
                let (_, scale) = decimal_precision_scale(field.data_type()).unwrap_or((38, 0));
                match value {
                    Value::Double(d) => {
                        let exact = BigDecimal::from_f64(*d).ok_or_else(|| {
                            FederationError::Arithmetic(format!("Infinite or NaN: {d}"))
                        })?;
                        Value::Decimal(set_scale_half_up(&exact, scale))
                    }
                    Value::Decimal(d) => Value::Decimal(set_scale_half_up(d, scale)),
                    other => return Err(class_cast(other, "java.math.BigDecimal")),
                }
            }
            MinorType::VARCHAR => match value {
                Value::String(s) => Value::String(s.clone()),
                Value::Bytes(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
                _ => return Ok(None),
            },
            MinorType::BIT => {
                let bit = matches!(value, Value::Int(i) if *i > 0)
                    || matches!(value, Value::Boolean(true));
                Value::Boolean(bit)
            }
            other => return Err(unknown_simple_type(field, other)),
        }))
    }

    pub fn unset_row(row: usize, block: &mut Block) -> Result<()> {
        for vector in block.field_vectors_mut() {
            match vector.minor_type() {
                MinorType::TIMESTAMPMICROTZ
                | MinorType::TIMESTAMPMILLITZ
                | MinorType::DATEDAY
                | MinorType::DATEMILLI
                | MinorType::TINYINT
                | MinorType::UINT1
                | MinorType::SMALLINT
                | MinorType::UINT2
                | MinorType::UINT4
                | MinorType::INT
                | MinorType::UINT8
                | MinorType::BIGINT
                | MinorType::FLOAT4
                | MinorType::FLOAT8
                | MinorType::DECIMAL
                | MinorType::VARBINARY
                | MinorType::VARCHAR
                | MinorType::BIT
                | MinorType::STRUCT
                | MinorType::MAP => vector.set_null(row),
                MinorType::LIST => {
                    vector.start_list(row)?;
                    let start = vector.list_slot(row).map_or(0, |(s, _)| s);
                    vector.end_list(row, start)?;
                }
                other => {
                    return Err(FederationError::invalid_input(format!(
                        "Unknown type {other}"
                    )))
                }
            }
        }
        Ok(())
    }

    pub fn get_java_type(minor_type: MinorType) -> Result<&'static str> {
        Ok(match minor_type {
            MinorType::TIMESTAMPMICROTZ | MinorType::TIMESTAMPMILLITZ => "java.time.ZonedDateTime",
            MinorType::DATEMILLI => "java.time.LocalDateTime",
            MinorType::TINYINT | MinorType::UINT1 => "java.lang.Byte",
            MinorType::SMALLINT => "java.lang.Short",
            MinorType::UINT2 => "java.lang.Character",
            MinorType::DATEDAY => "java.time.LocalDate",
            MinorType::INT | MinorType::UINT4 => "java.lang.Integer",
            MinorType::UINT8 | MinorType::BIGINT => "java.lang.Long",
            MinorType::DECIMAL => "java.math.BigDecimal",
            MinorType::FLOAT4 => "java.lang.Float",
            MinorType::FLOAT8 => "java.lang.Double",
            MinorType::VARCHAR => "java.lang.String",
            MinorType::VARBINARY => "[B",
            MinorType::BIT => "java.lang.Boolean",
            MinorType::LIST => "java.util.List",
            MinorType::STRUCT => "java.util.Map",
            other => {
                return Err(FederationError::invalid_input(format!(
                    "Unknown type {other}"
                )))
            }
        })
    }

    pub fn float_to_java_string(value: f32) -> String {
        float_to_string(value)
    }
}
