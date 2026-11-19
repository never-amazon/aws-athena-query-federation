use std::fmt;
use std::sync::Arc;

use arrow::datatypes::DataType;
use bigdecimal::{BigDecimal, RoundingMode};

use crate::data::java_time::local_date_time_of_epoch_milli_utc;
use crate::data::types::{decimal_precision_scale, MinorType};
use crate::data::value::Value;
use crate::exceptions::Result;

pub type Extract<C, T> = Arc<dyn Fn(&C) -> Result<Option<T>> + Send + Sync>;

pub enum Extractor<C> {
    BigInt(Extract<C, i64>),
    Bit(Extract<C, i32>),
    DateDay(Extract<C, i32>),
    DateMilli(Extract<C, i64>),
    Decimal(Extract<C, BigDecimal>),
    Float4(Extract<C, f32>),
    Float8(Extract<C, f64>),
    Int(Extract<C, i32>),
    SmallInt(Extract<C, i16>),
    TinyInt(Extract<C, i8>),
    VarBinary(Extract<C, Vec<u8>>),
    VarChar(Extract<C, String>),
}

impl<C> Extractor<C> {
    pub fn null(minor_type: MinorType) -> Option<Extractor<C>> {
        Some(match minor_type {
            MinorType::BigInt => Extractor::BigInt(Arc::new(|_: &C| Ok(None))),
            MinorType::Bit => Extractor::Bit(Arc::new(|_: &C| Ok(None))),
            MinorType::DateDay => Extractor::DateDay(Arc::new(|_: &C| Ok(None))),
            MinorType::DateMilli => Extractor::DateMilli(Arc::new(|_: &C| Ok(None))),
            MinorType::Decimal => Extractor::Decimal(Arc::new(|_: &C| Ok(None))),
            MinorType::Float4 => Extractor::Float4(Arc::new(|_: &C| Ok(None))),
            MinorType::Float8 => Extractor::Float8(Arc::new(|_: &C| Ok(None))),
            MinorType::Int => Extractor::Int(Arc::new(|_: &C| Ok(None))),
            MinorType::SmallInt => Extractor::SmallInt(Arc::new(|_: &C| Ok(None))),
            MinorType::TinyInt => Extractor::TinyInt(Arc::new(|_: &C| Ok(None))),
            MinorType::VarBinary => Extractor::VarBinary(Arc::new(|_: &C| Ok(None))),
            MinorType::VarChar => Extractor::VarChar(Arc::new(|_: &C| Ok(None))),
            _ => return None,
        })
    }

    pub fn minor_type(&self) -> MinorType {
        match self {
            Extractor::BigInt(_) => MinorType::BigInt,
            Extractor::Bit(_) => MinorType::Bit,
            Extractor::DateDay(_) => MinorType::DateDay,
            Extractor::DateMilli(_) => MinorType::DateMilli,
            Extractor::Decimal(_) => MinorType::Decimal,
            Extractor::Float4(_) => MinorType::Float4,
            Extractor::Float8(_) => MinorType::Float8,
            Extractor::Int(_) => MinorType::Int,
            Extractor::SmallInt(_) => MinorType::SmallInt,
            Extractor::TinyInt(_) => MinorType::TinyInt,
            Extractor::VarBinary(_) => MinorType::VarBinary,
            Extractor::VarChar(_) => MinorType::VarChar,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Extractor::BigInt(_) => "BigIntExtractor",
            Extractor::Bit(_) => "BitExtractor",
            Extractor::DateDay(_) => "DateDayExtractor",
            Extractor::DateMilli(_) => "DateMilliExtractor",
            Extractor::Decimal(_) => "DecimalExtractor",
            Extractor::Float4(_) => "Float4Extractor",
            Extractor::Float8(_) => "Float8Extractor",
            Extractor::Int(_) => "IntExtractor",
            Extractor::SmallInt(_) => "SmallIntExtractor",
            Extractor::TinyInt(_) => "TinyIntExtractor",
            Extractor::VarBinary(_) => "VarBinaryExtractor",
            Extractor::VarChar(_) => "VarCharExtractor",
        }
    }

    pub fn extract_values(
        &self,
        context: &C,
        vector_type: &DataType,
    ) -> Result<Option<(Value, Value)>> {
        Ok(match self {
            Extractor::BigInt(f) => f(context)?.map(|v| (Value::Long(v), Value::Long(v))),
            Extractor::Bit(f) => f(context)?.map(|v| (Value::Int(v), Value::Boolean(v > 0))),
            Extractor::DateDay(f) => f(context)?.map(|v| (Value::Int(v), Value::Int(v))),
            Extractor::DateMilli(f) => f(context)?.map(|v| {
                (
                    Value::Long(v),
                    Value::LocalDateTime(local_date_time_of_epoch_milli_utc(v)),
                )
            }),
            Extractor::Decimal(f) => f(context)?.map(|v| {
                let (_, scale) = decimal_precision_scale(vector_type).unwrap_or((38, 0));
                let scaled = v.with_scale_round(i64::from(scale), RoundingMode::HalfUp);
                (Value::Decimal(scaled), Value::Decimal(v))
            }),
            Extractor::Float4(f) => f(context)?.map(|v| (Value::Float(v), Value::Float(v))),
            Extractor::Float8(f) => f(context)?.map(|v| (Value::Double(v), Value::Double(v))),
            Extractor::Int(f) => f(context)?.map(|v| (Value::Int(v), Value::Int(v))),
            Extractor::SmallInt(f) => f(context)?.map(|v| (Value::Short(v), Value::Short(v))),
            Extractor::TinyInt(f) => f(context)?.map(|v| (Value::Byte(v), Value::Byte(v))),
            Extractor::VarBinary(f) => {
                f(context)?.map(|v| (Value::Bytes(v.clone()), Value::Bytes(v)))
            }
            Extractor::VarChar(f) => {
                f(context)?.map(|v| (Value::String(v.clone()), Value::String(v)))
            }
        })
    }
}

impl<C> Clone for Extractor<C> {
    fn clone(&self) -> Self {
        match self {
            Extractor::BigInt(f) => Extractor::BigInt(Arc::clone(f)),
            Extractor::Bit(f) => Extractor::Bit(Arc::clone(f)),
            Extractor::DateDay(f) => Extractor::DateDay(Arc::clone(f)),
            Extractor::DateMilli(f) => Extractor::DateMilli(Arc::clone(f)),
            Extractor::Decimal(f) => Extractor::Decimal(Arc::clone(f)),
            Extractor::Float4(f) => Extractor::Float4(Arc::clone(f)),
            Extractor::Float8(f) => Extractor::Float8(Arc::clone(f)),
            Extractor::Int(f) => Extractor::Int(Arc::clone(f)),
            Extractor::SmallInt(f) => Extractor::SmallInt(Arc::clone(f)),
            Extractor::TinyInt(f) => Extractor::TinyInt(Arc::clone(f)),
            Extractor::VarBinary(f) => Extractor::VarBinary(Arc::clone(f)),
            Extractor::VarChar(f) => Extractor::VarChar(Arc::clone(f)),
        }
    }
}

impl<C> fmt::Debug for Extractor<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.type_name())
    }
}
