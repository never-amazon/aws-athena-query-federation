use std::sync::Arc;

use arrow::array::{
    new_null_array, Array, ArrayAccessor, ArrayRef, BinaryArray, BooleanArray, Date32Array,
    Date64Array, Decimal128Array, Decimal256Array, FixedSizeBinaryArray, Float32Array,
    Float64Array, Int16Array, Int32Array, Int64Array, Int8Array, LargeBinaryArray,
    LargeStringArray, NullArray, StringArray, StringViewArray, Time32MillisecondArray,
    Time32SecondArray, Time64MicrosecondArray, Time64NanosecondArray, TimestampMicrosecondArray,
    TimestampMillisecondArray, TimestampNanosecondArray, TimestampSecondArray, UInt16Array,
    UInt32Array, UInt64Array, UInt8Array,
};
use arrow::datatypes::{i256, DataType, FieldRef, TimeUnit};
use bigdecimal::BigDecimal;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::types::MinorType;
use super::value::Value;
use super::vector::big_decimal_to_unscaled;
use crate::exceptions::{FederationError, Result};

#[derive(Debug, Clone)]
pub(crate) enum Column {
    Untyped(usize),
    Bool(Vec<Option<bool>>),
    Byte(Vec<Option<i8>>),
    Short(Vec<Option<i16>>),
    Char(Vec<Option<u16>>),
    Int(Vec<Option<i32>>),
    Long(Vec<Option<i64>>),
    Float(Vec<Option<f32>>),
    Double(Vec<Option<f64>>),
    Decimal(Vec<Option<BigDecimal>>),
    Utf8(Vec<Option<String>>),
    Binary(Vec<Option<Vec<u8>>>),
}

macro_rules! each_column {
    ($column:expr, $values:ident => $body:expr, $len:ident => $untyped:expr) => {
        match $column {
            Column::Untyped($len) => $untyped,
            Column::Bool($values) => $body,
            Column::Byte($values) => $body,
            Column::Short($values) => $body,
            Column::Char($values) => $body,
            Column::Int($values) => $body,
            Column::Long($values) => $body,
            Column::Float($values) => $body,
            Column::Double($values) => $body,
            Column::Decimal($values) => $body,
            Column::Utf8($values) => $body,
            Column::Binary($values) => $body,
        }
    };
}

fn grow<T: Clone>(values: &mut Vec<Option<T>>, index: usize) {
    if values.len() <= index {
        values.resize(index + 1, None);
    }
}

fn padded<T: Clone>(values: &[Option<T>], len: usize) -> Vec<Option<T>> {
    (0..len).map(|i| values.get(i).cloned().flatten()).collect()
}

macro_rules! collect_prim {
    ($array:expr, $t:ty, $f:expr) => {
        collect(downcast::<$t>($array, None)?, $f)
    };
}

impl Column {
    pub(crate) fn for_minor(minor: MinorType) -> Column {
        match minor {
            MinorType::Bit => Column::Bool(Vec::new()),
            MinorType::TinyInt | MinorType::UInt1 => Column::Byte(Vec::new()),
            MinorType::SmallInt => Column::Short(Vec::new()),
            MinorType::UInt2 => Column::Char(Vec::new()),
            MinorType::Int
            | MinorType::UInt4
            | MinorType::DateDay
            | MinorType::TimeSec
            | MinorType::TimeMilli => Column::Int(Vec::new()),
            MinorType::BigInt
            | MinorType::UInt8
            | MinorType::DateMilli
            | MinorType::TimeMicro
            | MinorType::TimeNano
            | MinorType::TimestampSec
            | MinorType::TimestampMilli
            | MinorType::TimestampMicro
            | MinorType::TimestampNano
            | MinorType::TimestampSecTz
            | MinorType::TimestampMilliTz
            | MinorType::TimestampMicroTz
            | MinorType::TimestampNanoTz => Column::Long(Vec::new()),
            MinorType::Float4 => Column::Float(Vec::new()),
            MinorType::Float8 => Column::Double(Vec::new()),
            MinorType::Decimal | MinorType::Decimal256 => Column::Decimal(Vec::new()),
            MinorType::VarChar | MinorType::LargeVarChar | MinorType::ViewVarChar => {
                Column::Utf8(Vec::new())
            }
            MinorType::VarBinary
            | MinorType::LargeVarBinary
            | MinorType::ViewVarBinary
            | MinorType::FixedSizeBinary => Column::Binary(Vec::new()),
            _ => Column::Untyped(0),
        }
    }

    pub(crate) fn len(&self) -> usize {
        each_column!(self, values => values.len(), len => *len)
    }

    pub(crate) fn is_set(&self, index: usize) -> bool {
        each_column!(self, values => values.get(index).is_some_and(Option::is_some), _len => false)
    }

    pub(crate) fn set_null(&mut self, index: usize) {
        each_column!(
            self,
            values => {
                grow(values, index);
                values[index] = None;
            },
            len => *len = (*len).max(index + 1)
        );
    }

    pub(crate) fn get(&self, index: usize) -> Value {
        let cell = |i: usize| -> Option<Value> {
            Some(match self {
                Column::Untyped(_) => return None,
                Column::Bool(v) => Value::Boolean((*v.get(i)?)?),
                Column::Byte(v) => Value::Byte((*v.get(i)?)?),
                Column::Short(v) => Value::Short((*v.get(i)?)?),
                Column::Char(v) => Value::Char((*v.get(i)?)?),
                Column::Int(v) => Value::Int((*v.get(i)?)?),
                Column::Long(v) => Value::Long((*v.get(i)?)?),
                Column::Float(v) => Value::Float((*v.get(i)?)?),
                Column::Double(v) => Value::Double((*v.get(i)?)?),
                Column::Decimal(v) => Value::Decimal(v.get(i)?.clone()?),
                Column::Utf8(v) => Value::String(v.get(i)?.clone()?),
                Column::Binary(v) => Value::Bytes(v.get(i)?.clone()?),
            })
        };
        cell(index).unwrap_or(Value::Null)
    }

    pub(crate) fn put(&mut self, index: usize, value: Value) -> Result<()> {
        fn store<T: Clone>(values: &mut Vec<Option<T>>, index: usize, v: T) {
            grow(values, index);
            values[index] = Some(v);
        }
        match (self, value) {
            (Column::Bool(v), Value::Boolean(x)) => store(v, index, x),
            (Column::Byte(v), Value::Byte(x)) => store(v, index, x),
            (Column::Short(v), Value::Short(x)) => store(v, index, x),
            (Column::Char(v), Value::Char(x)) => store(v, index, x),
            (Column::Int(v), Value::Int(x)) => store(v, index, x),
            (Column::Long(v), Value::Long(x)) => store(v, index, x),
            (Column::Float(v), Value::Float(x)) => store(v, index, x),
            (Column::Double(v), Value::Double(x)) => store(v, index, x),
            (Column::Decimal(v), Value::Decimal(x)) => store(v, index, x),
            (Column::Utf8(v), Value::String(x)) => store(v, index, x),
            (Column::Binary(v), Value::Bytes(x)) => store(v, index, x),
            (_, other) => {
                return Err(FederationError::UnsupportedOperation(format!(
                    "class {} does not match the column storage",
                    other.java_class_name()
                )))
            }
        }
        Ok(())
    }

    pub(crate) fn variable_width_bytes(&self, len: usize) -> usize {
        match self {
            Column::Utf8(v) => v.iter().take(len).flatten().map(String::len).sum(),
            Column::Binary(v) => v.iter().take(len).flatten().map(Vec::len).sum(),
            _ => 0,
        }
    }

    pub(crate) fn to_array(
        &self,
        field: &FieldRef,
        minor: MinorType,
        len: usize,
    ) -> Result<ArrayRef> {
        let unsupported = || {
            FederationError::not_supported(format!(
                "Unsupported type {} for field {}",
                minor,
                field.name()
            ))
        };
        let arr: ArrayRef = match (minor, self) {
            (MinorType::Null, _) => Arc::new(NullArray::new(len)),
            (MinorType::Bit, Column::Bool(v)) => Arc::new(BooleanArray::from(padded(v, len))),
            (MinorType::TinyInt, Column::Byte(v)) => Arc::new(Int8Array::from(padded(v, len))),
            (MinorType::UInt1, Column::Byte(v)) => Arc::new(UInt8Array::from(
                padded(v, len)
                    .into_iter()
                    .map(|x| x.map(i8::cast_unsigned))
                    .collect::<Vec<_>>(),
            )),
            (MinorType::SmallInt, Column::Short(v)) => Arc::new(Int16Array::from(padded(v, len))),
            (MinorType::UInt2, Column::Char(v)) => Arc::new(UInt16Array::from(padded(v, len))),
            (MinorType::Int, Column::Int(v)) => Arc::new(Int32Array::from(padded(v, len))),
            (MinorType::UInt4, Column::Int(v)) => Arc::new(UInt32Array::from(
                padded(v, len)
                    .into_iter()
                    .map(|x| x.map(i32::cast_unsigned))
                    .collect::<Vec<_>>(),
            )),
            (MinorType::BigInt, Column::Long(v)) => Arc::new(Int64Array::from(padded(v, len))),
            (MinorType::UInt8, Column::Long(v)) => Arc::new(UInt64Array::from(
                padded(v, len)
                    .into_iter()
                    .map(|x| x.map(i64::cast_unsigned))
                    .collect::<Vec<_>>(),
            )),
            (MinorType::Float4, Column::Float(v)) => Arc::new(Float32Array::from(padded(v, len))),
            (MinorType::Float8, Column::Double(v)) => Arc::new(Float64Array::from(padded(v, len))),
            (MinorType::DateDay, Column::Int(v)) => Arc::new(Date32Array::from(padded(v, len))),
            (MinorType::DateMilli, Column::Long(v)) => Arc::new(Date64Array::from(padded(v, len))),
            (MinorType::TimeSec, Column::Int(v)) => {
                Arc::new(Time32SecondArray::from(padded(v, len)))
            }
            (MinorType::TimeMilli, Column::Int(v)) => {
                Arc::new(Time32MillisecondArray::from(padded(v, len)))
            }
            (MinorType::TimeMicro, Column::Long(v)) => {
                Arc::new(Time64MicrosecondArray::from(padded(v, len)))
            }
            (MinorType::TimeNano, Column::Long(v)) => {
                Arc::new(Time64NanosecondArray::from(padded(v, len)))
            }
            (
                MinorType::TimestampSec
                | MinorType::TimestampMilli
                | MinorType::TimestampMicro
                | MinorType::TimestampNano
                | MinorType::TimestampSecTz
                | MinorType::TimestampMilliTz
                | MinorType::TimestampMicroTz
                | MinorType::TimestampNanoTz,
                Column::Long(v),
            ) => timestamp_array(field.data_type(), padded(v, len)),
            (MinorType::Decimal, Column::Decimal(v)) => {
                decimal128_array(field.data_type(), v, len)?
            }
            (MinorType::Decimal256, Column::Decimal(v)) => {
                decimal256_array(field.data_type(), v, len)?
            }
            (MinorType::VarChar, Column::Utf8(v)) => Arc::new(StringArray::from(padded(v, len))),
            (MinorType::LargeVarChar, Column::Utf8(v)) => {
                Arc::new(LargeStringArray::from(padded(v, len)))
            }
            (MinorType::VarBinary, Column::Binary(v)) => Arc::new(BinaryArray::from(
                (0..len)
                    .map(|i| v.get(i).and_then(|b| b.as_deref()))
                    .collect::<Vec<_>>(),
            )),
            (MinorType::LargeVarBinary, Column::Binary(v)) => Arc::new(LargeBinaryArray::from(
                (0..len)
                    .map(|i| v.get(i).and_then(|b| b.as_deref()))
                    .collect::<Vec<_>>(),
            )),
            (MinorType::FixedSizeBinary, Column::Binary(v)) => {
                let size = match field.data_type() {
                    DataType::FixedSizeBinary(n) => *n,
                    _ => 0,
                };
                let values = padded(v, len);
                if values.iter().all(Option::is_none) {
                    new_null_array(field.data_type(), len)
                } else {
                    Arc::new(FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        values.into_iter(),
                        size,
                    )?)
                }
            }
            _ => return Err(unsupported()),
        };
        Ok(arr)
    }

    pub(crate) fn from_array(minor: MinorType, array: &dyn Array) -> Result<Column> {
        Ok(match minor {
            MinorType::Null => Column::Untyped(array.len()),
            MinorType::Bit => Column::Bool(collect_prim!(array, BooleanArray, |v| v)),
            MinorType::TinyInt => Column::Byte(collect_prim!(array, Int8Array, |v| v)),
            MinorType::UInt1 => Column::Byte(collect_prim!(array, UInt8Array, u8::cast_signed)),
            MinorType::SmallInt => Column::Short(collect_prim!(array, Int16Array, |v| v)),
            MinorType::UInt2 => Column::Char(collect_prim!(array, UInt16Array, |v| v)),
            MinorType::Int => Column::Int(collect_prim!(array, Int32Array, |v| v)),
            MinorType::UInt4 => Column::Int(collect_prim!(array, UInt32Array, u32::cast_signed)),
            MinorType::BigInt => Column::Long(collect_prim!(array, Int64Array, |v| v)),
            MinorType::UInt8 => Column::Long(collect_prim!(array, UInt64Array, u64::cast_signed)),
            MinorType::Float4 => Column::Float(collect_prim!(array, Float32Array, |v| v)),
            MinorType::Float8 => Column::Double(collect_prim!(array, Float64Array, |v| v)),
            MinorType::DateDay => Column::Int(collect_prim!(array, Date32Array, |v| v)),
            MinorType::DateMilli => Column::Long(collect_prim!(array, Date64Array, |v| v)),
            MinorType::TimeSec => Column::Int(collect_prim!(array, Time32SecondArray, |v| v)),
            MinorType::TimeMilli => {
                Column::Int(collect_prim!(array, Time32MillisecondArray, |v| v))
            }
            MinorType::TimeMicro => {
                Column::Long(collect_prim!(array, Time64MicrosecondArray, |v| v))
            }
            MinorType::TimeNano => Column::Long(collect_prim!(array, Time64NanosecondArray, |v| v)),
            MinorType::TimestampSec | MinorType::TimestampSecTz => {
                Column::Long(collect_prim!(array, TimestampSecondArray, |v| v))
            }
            MinorType::TimestampMilli | MinorType::TimestampMilliTz => {
                Column::Long(collect_prim!(array, TimestampMillisecondArray, |v| v))
            }
            MinorType::TimestampMicro | MinorType::TimestampMicroTz => {
                Column::Long(collect_prim!(array, TimestampMicrosecondArray, |v| v))
            }
            MinorType::TimestampNano | MinorType::TimestampNanoTz => {
                Column::Long(collect_prim!(array, TimestampNanosecondArray, |v| v))
            }
            MinorType::Decimal => {
                let a = downcast::<Decimal128Array>(array, Some("expected decimal128 array"))?;
                let scale = i64::from(a.scale());
                Column::Decimal(collect(a, |v| BigDecimal::new(BigInt::from(v), scale)))
            }
            MinorType::Decimal256 => {
                let a = downcast::<Decimal256Array>(array, Some("expected decimal256 array"))?;
                let scale = i64::from(a.scale());
                Column::Decimal(collect(a, |v| {
                    BigDecimal::new(BigInt::from_signed_bytes_le(&v.to_le_bytes()), scale)
                }))
            }
            MinorType::VarChar => Column::Utf8(collect_prim!(array, StringArray, str::to_string)),
            MinorType::LargeVarChar => {
                Column::Utf8(collect_prim!(array, LargeStringArray, str::to_string))
            }
            MinorType::ViewVarChar => {
                Column::Utf8(collect_prim!(array, StringViewArray, str::to_string))
            }
            MinorType::VarBinary => {
                Column::Binary(collect_prim!(array, BinaryArray, <[u8]>::to_vec))
            }
            MinorType::LargeVarBinary => {
                Column::Binary(collect_prim!(array, LargeBinaryArray, <[u8]>::to_vec))
            }
            MinorType::FixedSizeBinary => {
                Column::Binary(collect_prim!(array, FixedSizeBinaryArray, <[u8]>::to_vec))
            }
            other => {
                return Err(FederationError::not_supported(format!(
                    "Unsupported type {other}"
                )));
            }
        })
    }
}

fn downcast<'a, T: 'static>(array: &'a dyn Array, message: Option<&str>) -> Result<&'a T> {
    array.as_any().downcast_ref::<T>().ok_or_else(|| {
        FederationError::invalid_input(message.map_or_else(
            || format!("unexpected array type {:?}", array.data_type()),
            str::to_string,
        ))
    })
}

fn collect<A: ArrayAccessor + Copy, T>(a: A, f: impl Fn(A::Item) -> T) -> Vec<Option<T>> {
    (0..a.len())
        .map(|i| a.is_valid(i).then(|| f(a.value(i))))
        .collect()
}

fn timestamp_array(data_type: &DataType, v: Vec<Option<i64>>) -> ArrayRef {
    let (unit, tz) = match data_type {
        DataType::Timestamp(u, tz) => (*u, tz.clone()),
        _ => (TimeUnit::Millisecond, None),
    };
    match unit {
        TimeUnit::Second => Arc::new(TimestampSecondArray::from(v).with_timezone_opt(tz)),
        TimeUnit::Millisecond => Arc::new(TimestampMillisecondArray::from(v).with_timezone_opt(tz)),
        TimeUnit::Microsecond => Arc::new(TimestampMicrosecondArray::from(v).with_timezone_opt(tz)),
        TimeUnit::Nanosecond => Arc::new(TimestampNanosecondArray::from(v).with_timezone_opt(tz)),
    }
}

fn decimal128_array(
    data_type: &DataType,
    values: &[Option<BigDecimal>],
    len: usize,
) -> Result<ArrayRef> {
    let (p, s) = super::types::decimal_precision_scale(data_type).unwrap_or((38, 0));
    let v: Vec<Option<i128>> = (0..len)
        .map(|i| {
            values
                .get(i)
                .and_then(Option::as_ref)
                .and_then(|d| big_decimal_to_unscaled(d, s).to_i128())
        })
        .collect();
    Ok(Arc::new(
        Decimal128Array::from(v).with_precision_and_scale(p, s)?,
    ))
}

fn decimal256_array(
    data_type: &DataType,
    values: &[Option<BigDecimal>],
    len: usize,
) -> Result<ArrayRef> {
    let (p, s) = super::types::decimal_precision_scale(data_type).unwrap_or((76, 0));
    let v: Vec<Option<i256>> = (0..len)
        .map(|i| {
            values.get(i).and_then(Option::as_ref).map(|d| {
                let bytes = big_decimal_to_unscaled(d, s).to_signed_bytes_le();
                let mut buf = if bytes.last().is_some_and(|b| b & 0x80 != 0) {
                    [0xffu8; 32]
                } else {
                    [0u8; 32]
                };
                let n = bytes.len().min(32);
                buf[..n].copy_from_slice(&bytes[..n]);
                i256::from_le_bytes(buf)
            })
        })
        .collect();
    Ok(Arc::new(
        Decimal256Array::from(v).with_precision_and_scale(p, s)?,
    ))
}
