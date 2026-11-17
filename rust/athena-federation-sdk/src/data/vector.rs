use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use arrow::array::{
    new_null_array, Array, ArrayAccessor, ArrayRef, BinaryArray, BooleanArray, Date32Array,
    Date64Array, Decimal128Array, Decimal256Array, FixedSizeBinaryArray, Float32Array,
    Float64Array, Int16Array, Int32Array, Int64Array, Int8Array, LargeBinaryArray,
    LargeStringArray, ListArray, MapArray, NullArray, StringArray, StringViewArray, StructArray,
    Time32MillisecondArray, Time32SecondArray, Time64MicrosecondArray, Time64NanosecondArray,
    TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray,
    TimestampSecondArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use arrow::buffer::{NullBuffer, OffsetBuffer};
use arrow::datatypes::{i256, DataType, FieldRef, Fields, TimeUnit};
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::java_time::local_date_time_of_epoch_milli_utc;
use super::types::{minor_type_of, MinorType};
use super::value::Value;
use crate::exceptions::{FederationError, Result};
use crate::java_cast::{
    double_to_float, long_to_byte, long_to_char, long_to_double, long_to_float, long_to_int,
    long_to_short,
};

#[derive(Debug, Clone)]
enum VectorData {
    Scalar(Vec<Value>),
    Struct {
        validity: Vec<bool>,
        children: Vec<FieldVector>,
    },
    List {
        slots: Vec<Option<(usize, usize)>>,
        child: Box<FieldVector>,
    },
}

#[derive(Debug, Clone)]
pub struct FieldVector {
    field: FieldRef,
    minor_type: MinorType,
    value_count: usize,
    data: VectorData,
}

fn class_cast(vector: &FieldVector, value: &Value) -> FederationError {
    FederationError::ClassCast(format!(
        "class {} cannot be cast to the storage type of {} vector {}",
        value.java_class_name(),
        vector.minor_type,
        vector.field.name()
    ))
}

pub fn big_decimal_to_unscaled(value: &BigDecimal, scale: i8) -> BigInt {
    let (unscaled, exp) = value
        .with_scale(i64::from(scale))
        .into_bigint_and_exponent();
    debug_assert_eq!(exp, i64::from(scale));
    unscaled
}

pub fn decimal_precision(value: &BigDecimal) -> u64 {
    let (unscaled, _) = value.as_bigint_and_exponent();
    let digits = unscaled.magnitude().to_string();
    if digits == "0" {
        1
    } else {
        digits.len() as u64
    }
}

impl FieldVector {
    pub fn new(field: FieldRef) -> FieldVector {
        let minor_type = minor_type_of(field.data_type());
        let data = match field.data_type() {
            DataType::Struct(children) => VectorData::Struct {
                validity: Vec::new(),
                children: children
                    .iter()
                    .map(|c| FieldVector::new(c.clone()))
                    .collect(),
            },
            DataType::List(child) | DataType::LargeList(child) | DataType::Map(child, _) => {
                VectorData::List {
                    slots: Vec::new(),
                    child: Box::new(FieldVector::new(child.clone())),
                }
            }
            _ => VectorData::Scalar(Vec::new()),
        };
        FieldVector {
            field,
            minor_type,
            value_count: 0,
            data,
        }
    }

    pub fn field(&self) -> &FieldRef {
        &self.field
    }

    pub fn name(&self) -> &str {
        self.field.name()
    }

    pub fn minor_type(&self) -> MinorType {
        self.minor_type
    }

    pub fn value_count(&self) -> usize {
        self.value_count
    }

    pub fn set_value_count(&mut self, count: usize) {
        self.value_count = count;
        if let VectorData::Struct { children, .. } = &mut self.data {
            for c in children.iter_mut() {
                c.set_value_count(count);
            }
        }
    }

    pub fn reset(&mut self) {
        let fresh = FieldVector::new(self.field.clone());
        *self = fresh;
    }

    pub fn children(&self) -> Vec<&FieldVector> {
        match &self.data {
            VectorData::Struct { children, .. } => children.iter().collect(),
            VectorData::List { child, .. } => vec![child.as_ref()],
            VectorData::Scalar(_) => Vec::new(),
        }
    }

    pub fn get_child(&self, name: &str) -> Option<&FieldVector> {
        match &self.data {
            VectorData::Struct { children, .. } => children.iter().find(|c| c.name() == name),
            _ => None,
        }
    }

    pub fn get_child_mut(&mut self, name: &str) -> Option<&mut FieldVector> {
        match &mut self.data {
            VectorData::Struct { children, .. } => children.iter_mut().find(|c| c.name() == name),
            _ => None,
        }
    }

    pub fn data_vector(&self) -> Option<&FieldVector> {
        match &self.data {
            VectorData::List { child, .. } => Some(child.as_ref()),
            _ => None,
        }
    }

    pub fn data_vector_mut(&mut self) -> Option<&mut FieldVector> {
        match &mut self.data {
            VectorData::List { child, .. } => Some(child.as_mut()),
            _ => None,
        }
    }

    pub fn is_complex(&self) -> bool {
        !matches!(self.data, VectorData::Scalar(_))
    }

    pub fn is_null(&self, index: usize) -> bool {
        !self.is_set(index)
    }

    pub fn is_set(&self, index: usize) -> bool {
        match &self.data {
            VectorData::Scalar(values) => values.get(index).is_some_and(|v| !v.is_null()),
            VectorData::Struct { validity, .. } => validity.get(index).copied().unwrap_or(false),
            VectorData::List { slots, .. } => {
                slots.get(index).is_some_and(std::option::Option::is_some)
            }
        }
    }

    pub fn null_count(&self) -> usize {
        (0..self.value_count).filter(|i| self.is_null(*i)).count()
    }

    pub(crate) fn list_slot(&self, index: usize) -> Option<(usize, usize)> {
        match &self.data {
            VectorData::List { slots, .. } => slots.get(index).copied().flatten(),
            _ => None,
        }
    }

    pub fn get_raw(&self, index: usize) -> Value {
        match &self.data {
            VectorData::Scalar(values) => values.get(index).cloned().unwrap_or(Value::Null),
            _ => self.get_object(index),
        }
    }

    pub fn get_object(&self, index: usize) -> Value {
        match &self.data {
            VectorData::Scalar(values) => {
                let Some(raw) = values.get(index) else {
                    return Value::Null;
                };
                match (self.minor_type, raw) {
                    (MinorType::DATEMILLI | MinorType::TIMESTAMPMILLI, Value::Long(ms)) => {
                        local_date_time_of_epoch_milli_utc(*ms)
                            .map_or(Value::Null, Value::LocalDateTime)
                    }
                    (MinorType::TIMESTAMPSEC, Value::Long(s)) => {
                        chrono::DateTime::from_timestamp(*s, 0)
                            .map_or(Value::Null, |d| Value::LocalDateTime(d.naive_utc()))
                    }
                    (MinorType::TIMESTAMPMICRO, Value::Long(us)) => {
                        chrono::DateTime::from_timestamp_micros(*us)
                            .map_or(Value::Null, |d| Value::LocalDateTime(d.naive_utc()))
                    }
                    (MinorType::TIMESTAMPNANO, Value::Long(ns)) => Value::LocalDateTime(
                        chrono::DateTime::from_timestamp_nanos(*ns).naive_utc(),
                    ),
                    _ => raw.clone(),
                }
            }
            VectorData::Struct { validity, children } => {
                if !validity.get(index).copied().unwrap_or(false) {
                    return Value::Null;
                }
                let mut entries = Vec::new();
                for child in children {
                    let v = child.get_object(index);
                    if !v.is_null() {
                        entries.push((Value::String(child.name().to_string()), v));
                    }
                }
                Value::Map(entries)
            }
            VectorData::List { slots, child } => match slots.get(index).copied().flatten() {
                None => Value::Null,
                Some((start, end)) => {
                    Value::List((start..end).map(|i| child.get_object(i)).collect())
                }
            },
        }
    }

    pub fn get_local_date_time(&self, index: usize) -> Option<NaiveDateTime> {
        match self.get_object(index) {
            Value::LocalDateTime(v) => Some(v),
            _ => None,
        }
    }

    fn scalar_slot(&mut self, index: usize) -> Result<&mut Value> {
        match &mut self.data {
            VectorData::Scalar(values) => {
                if values.len() <= index {
                    values.resize(index + 1, Value::Null);
                }
                Ok(&mut values[index])
            }
            _ => Err(FederationError::UnsupportedOperation(format!(
                "vector {} is not a scalar vector",
                self.field.name()
            ))),
        }
    }

    pub fn set_null(&mut self, index: usize) {
        match &mut self.data {
            VectorData::Scalar(values) => {
                if values.len() <= index {
                    values.resize(index + 1, Value::Null);
                }
                values[index] = Value::Null;
            }
            VectorData::Struct { validity, children } => {
                if validity.len() <= index {
                    validity.resize(index + 1, false);
                }
                validity[index] = false;
                for c in children.iter_mut() {
                    c.set_null(index);
                }
            }
            VectorData::List { slots, .. } => {
                if slots.len() <= index {
                    slots.resize(index + 1, None);
                }
                slots[index] = None;
            }
        }
    }

    pub fn set_index_defined(&mut self, index: usize) {
        match &mut self.data {
            VectorData::Struct { validity, .. } => {
                if validity.len() <= index {
                    validity.resize(index + 1, false);
                }
                validity[index] = true;
            }
            VectorData::List { slots, child } => {
                if slots.len() <= index {
                    slots.resize(index + 1, None);
                }
                if slots[index].is_none() {
                    let end = child.stored_len().max(child.value_count);
                    slots[index] = Some((end, end));
                }
            }
            VectorData::Scalar(_) => {}
        }
    }

    pub(crate) fn stored_len(&self) -> usize {
        match &self.data {
            VectorData::Scalar(values) => values.len(),
            VectorData::Struct { validity, children } => children
                .iter()
                .map(FieldVector::stored_len)
                .chain(std::iter::once(validity.len()))
                .max()
                .unwrap_or(0),
            VectorData::List { slots, .. } => slots.len(),
        }
    }

    pub fn set_safe(&mut self, index: usize, value: impl Into<Value>) -> Result<()> {
        let value = value.into();
        self.set(index, &value)
    }

    pub fn set(&mut self, index: usize, value: &Value) -> Result<()> {
        if value.is_null() {
            self.set_null(index);
            return Ok(());
        }
        if self.is_complex() {
            return self.set_complex_object(index, value);
        }
        let minor = self.minor_type;
        let field = self.field.clone();
        let integral = || integral_bits(value).ok_or_else(|| class_cast(self, value));
        let canonical = match minor {
            MinorType::BIT => match value {
                Value::Boolean(b) => Value::Boolean(*b),
                Value::Int(i) => Value::Boolean(*i != 0),
                Value::Long(i) => Value::Boolean(*i != 0),
                _ => return Err(class_cast(self, value)),
            },
            MinorType::TINYINT | MinorType::UINT1 => Value::Byte(long_to_byte(integral()?)),
            MinorType::SMALLINT => Value::Short(long_to_short(integral()?)),
            MinorType::UINT2 => Value::Char(long_to_char(integral()?)),
            MinorType::INT
            | MinorType::UINT4
            | MinorType::DATEDAY
            | MinorType::TIMESEC
            | MinorType::TIMEMILLI => Value::Int(long_to_int(integral()?)),
            MinorType::BIGINT
            | MinorType::UINT8
            | MinorType::DATEMILLI
            | MinorType::TIMEMICRO
            | MinorType::TIMENANO
            | MinorType::TIMESTAMPSEC
            | MinorType::TIMESTAMPMILLI
            | MinorType::TIMESTAMPMICRO
            | MinorType::TIMESTAMPNANO
            | MinorType::TIMESTAMPSECTZ
            | MinorType::TIMESTAMPMILLITZ
            | MinorType::TIMESTAMPMICROTZ
            | MinorType::TIMESTAMPNANOTZ => match integral_bits(value) {
                Some(v) => Value::Long(v),
                None => match value {
                    Value::LocalDateTime(dt)
                        if minor == MinorType::DATEMILLI || minor == MinorType::TIMESTAMPMILLI =>
                    {
                        Value::Long(dt.and_utc().timestamp_millis())
                    }
                    _ => return Err(class_cast(self, value)),
                },
            },
            MinorType::FLOAT4 => match value {
                Value::Float(v) => Value::Float(*v),
                Value::Double(v) => Value::Float(double_to_float(*v)),
                _ => Value::Float(long_to_float(integral()?)),
            },
            MinorType::FLOAT8 => match value {
                Value::Float(v) => Value::Double(f64::from(*v)),
                Value::Double(v) => Value::Double(*v),
                _ => Value::Double(long_to_double(integral()?)),
            },
            MinorType::DECIMAL | MinorType::DECIMAL256 => match value {
                Value::Decimal(d) => Value::Decimal(checked_decimal(field.data_type(), d)?),
                _ => return Err(class_cast(self, value)),
            },
            MinorType::VARCHAR | MinorType::LARGEVARCHAR | MinorType::VIEWVARCHAR => match value {
                Value::String(s) => Value::String(s.clone()),
                Value::Bytes(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
                _ => return Err(class_cast(self, value)),
            },
            MinorType::VARBINARY
            | MinorType::LARGEVARBINARY
            | MinorType::VIEWVARBINARY
            | MinorType::FIXEDSIZEBINARY => match value {
                Value::Bytes(b) => Value::Bytes(b.clone()),
                _ => return Err(class_cast(self, value)),
            },
            _ => {
                return Err(FederationError::UnsupportedOperation(format!(
                    "Unsupported vector type {} for field {}",
                    minor,
                    field.name()
                )))
            }
        };
        *self.scalar_slot(index)? = canonical;
        Ok(())
    }

    pub fn set_complex_object(&mut self, index: usize, value: &Value) -> Result<()> {
        if value.is_null() {
            self.set_null(index);
            return Ok(());
        }
        match &mut self.data {
            VectorData::Scalar(_) => self.set(index, value),
            VectorData::Struct { validity, children } => {
                let entries = match value {
                    Value::Map(entries) => entries,
                    other => {
                        return Err(FederationError::ClassCast(format!(
                            "class {} cannot be written to struct field",
                            other.java_class_name()
                        )))
                    }
                };
                if validity.len() <= index {
                    validity.resize(index + 1, false);
                }
                validity[index] = true;
                for child in children.iter_mut() {
                    let name = child.name().to_string();
                    let v = entries
                        .iter()
                        .find(|(k, _)| matches!(k, Value::String(s) if *s == name))
                        .map_or(Value::Null, |(_, v)| v.clone());
                    child.set_complex_object(index, &v)?;
                }
                Ok(())
            }
            VectorData::List { slots, child } => {
                let is_map = matches!(self.field.data_type(), DataType::Map(_, _));
                let items: Vec<Value> = match value {
                    Value::List(items) => items.clone(),
                    Value::Map(entries) if is_map => entries
                        .iter()
                        .map(|(k, v)| {
                            Value::Map(vec![
                                (Value::String("key".into()), k.clone()),
                                (Value::String("value".into()), v.clone()),
                            ])
                        })
                        .collect(),
                    other => {
                        return Err(FederationError::ClassCast(format!(
                            "class {} cannot be written to list field",
                            other.java_class_name()
                        )))
                    }
                };
                let start = child.stored_len();
                for (i, item) in items.iter().enumerate() {
                    child.set_complex_object(start + i, item)?;
                }
                let end = start + items.len();
                child.set_value_count(end);
                if slots.len() <= index {
                    slots.resize(index + 1, None);
                }
                slots[index] = Some((start, end));
                Ok(())
            }
        }
    }

    pub fn start_new_value(&mut self, index: usize) -> Result<usize> {
        self.start_list(index)
    }

    pub fn end_value(&mut self, index: usize, size: usize) -> Result<()> {
        let start = self.list_slot(index).map_or(0, |(s, _)| s);
        self.end_list(index, start + size)
    }

    pub(crate) fn start_list(&mut self, index: usize) -> Result<usize> {
        match &mut self.data {
            VectorData::List { slots, child } => {
                if slots.len() <= index {
                    slots.resize(index + 1, None);
                }
                let start = child.stored_len().max(child.value_count);
                slots[index] = Some((start, start));
                Ok(start)
            }
            _ => Err(FederationError::UnsupportedOperation(
                "not a list vector".into(),
            )),
        }
    }

    pub(crate) fn end_list(&mut self, index: usize, end: usize) -> Result<()> {
        match &mut self.data {
            VectorData::List { slots, child } => {
                if let Some(Some((start, _))) = slots.get(index).copied() {
                    slots[index] = Some((start, end));
                }
                if child.value_count < end {
                    child.set_value_count(end);
                }
                Ok(())
            }
            _ => Err(FederationError::UnsupportedOperation(
                "not a list vector".into(),
            )),
        }
    }

    pub fn get_buffer_size(&self) -> usize {
        let n = self.value_count;
        if n == 0 {
            return 0;
        }
        let validity = n.div_ceil(8);
        match &self.data {
            VectorData::Scalar(values) => {
                let width = match self.minor_type {
                    MinorType::BIT => 0,
                    MinorType::TINYINT | MinorType::UINT1 => 1,
                    MinorType::SMALLINT | MinorType::UINT2 => 2,
                    MinorType::INT
                    | MinorType::UINT4
                    | MinorType::FLOAT4
                    | MinorType::DATEDAY
                    | MinorType::VARCHAR
                    | MinorType::VARBINARY => 4,
                    MinorType::DECIMAL => 16,
                    MinorType::DECIMAL256 => 32,
                    _ => 8,
                };
                let var: usize = values
                    .iter()
                    .take(n)
                    .map(|v| match v {
                        Value::String(s) => s.len(),
                        Value::Bytes(b) => b.len(),
                        _ => 0,
                    })
                    .sum();
                let bits = if self.minor_type == MinorType::BIT {
                    validity
                } else {
                    0
                };
                let offsets =
                    if matches!(self.minor_type, MinorType::VARCHAR | MinorType::VARBINARY) {
                        4
                    } else {
                        0
                    };
                validity + bits + width * n + offsets + var
            }
            VectorData::Struct { children, .. } => {
                validity
                    + children
                        .iter()
                        .map(FieldVector::get_buffer_size)
                        .sum::<usize>()
            }
            VectorData::List { child, .. } => validity + 4 * (n + 1) + child.get_buffer_size(),
        }
    }

    pub fn to_array(&self) -> Result<ArrayRef> {
        self.to_array_len(self.value_count)
    }

    pub fn to_array_len(&self, len: usize) -> Result<ArrayRef> {
        match &self.data {
            VectorData::Scalar(values) => {
                scalar_to_array(&self.field, self.minor_type, values, len)
            }
            VectorData::Struct { validity, children } => {
                let fields = match self.field.data_type() {
                    DataType::Struct(f) => f.clone(),
                    _ => Fields::empty(),
                };
                if fields.is_empty() {
                    return Ok(Arc::new(StructArray::new_empty_fields(
                        len,
                        Some(NullBuffer::from(
                            (0..len)
                                .map(|i| validity.get(i).copied().unwrap_or(false))
                                .collect::<Vec<_>>(),
                        )),
                    )));
                }
                let arrays = children
                    .iter()
                    .map(|c| c.to_array_len(len))
                    .collect::<Result<Vec<_>>>()?;
                let nulls = NullBuffer::from(
                    (0..len)
                        .map(|i| validity.get(i).copied().unwrap_or(false))
                        .collect::<Vec<_>>(),
                );
                Ok(Arc::new(StructArray::try_new(fields, arrays, Some(nulls))?))
            }
            VectorData::List { slots, child } => {
                let mut offsets: Vec<i32> = Vec::with_capacity(len + 1);
                offsets.push(0);
                let mut indices: Vec<u32> = Vec::new();
                let mut identity = true;
                let mut valid = Vec::with_capacity(len);
                for i in 0..len {
                    let slot = slots.get(i).copied().flatten();
                    valid.push(slot.is_some());
                    if let Some((s, e)) = slot {
                        for j in s..e {
                            if j != indices.len() {
                                identity = false;
                            }
                            indices.push(u32::try_from(j).map_err(|_| list_overflow())?);
                        }
                    }
                    offsets.push(i32::try_from(indices.len()).map_err(|_| list_overflow())?);
                }
                let child_len = child.stored_len().max(child.value_count);
                let child_array = child.to_array_len(child_len)?;
                let values = if identity {
                    child_array.slice(0, indices.len())
                } else {
                    let idx = UInt32Array::from(indices);
                    arrow::compute::take(child_array.as_ref(), &idx, None)?
                };
                let nulls = NullBuffer::from(valid);
                let offsets = OffsetBuffer::new(offsets.into());
                match self.field.data_type() {
                    DataType::List(child_field) => Ok(Arc::new(ListArray::try_new(
                        child_field.clone(),
                        offsets,
                        values,
                        Some(nulls),
                    )?)),
                    DataType::Map(entries_field, sorted) => {
                        let entries = values
                            .as_any()
                            .downcast_ref::<StructArray>()
                            .cloned()
                            .ok_or_else(|| {
                                FederationError::invalid_input("map entries must be a struct")
                            })?;
                        let entries = StructArray::try_new(
                            entries.fields().clone(),
                            entries.columns().to_vec(),
                            None,
                        )?;
                        Ok(Arc::new(MapArray::try_new(
                            entries_field.clone(),
                            offsets,
                            entries,
                            Some(nulls),
                            *sorted,
                        )?))
                    }
                    other => Err(FederationError::not_supported(format!(
                        "Unsupported list type {other:?}"
                    ))),
                }
            }
        }
    }

    pub fn from_array(field: FieldRef, array: &dyn Array) -> Result<FieldVector> {
        let mut vector = FieldVector::new(field);
        vector.load_array(array)?;
        Ok(vector)
    }

    pub fn load_array(&mut self, array: &dyn Array) -> Result<()> {
        let len = array.len();
        match &mut self.data {
            VectorData::Scalar(values) => {
                *values = array_to_values(self.minor_type, array)?;
            }
            VectorData::Struct { validity, children } => {
                let sa = array
                    .as_any()
                    .downcast_ref::<StructArray>()
                    .ok_or_else(|| FederationError::invalid_input("expected struct array"))?;
                *validity = (0..len).map(|i| sa.is_valid(i)).collect();
                for (i, child) in children.iter_mut().enumerate() {
                    let col = sa.column(i);
                    child.load_array(col.as_ref())?;
                }
            }
            VectorData::List { slots, child } => {
                let (offsets, values, nulls): (Vec<i64>, ArrayRef, Vec<bool>) =
                    if let Some(la) = array.as_any().downcast_ref::<ListArray>() {
                        (
                            la.value_offsets().iter().map(|o| i64::from(*o)).collect(),
                            la.values().clone(),
                            (0..len).map(|i| la.is_valid(i)).collect(),
                        )
                    } else if let Some(ma) = array.as_any().downcast_ref::<MapArray>() {
                        (
                            ma.value_offsets().iter().map(|o| i64::from(*o)).collect(),
                            Arc::new(ma.entries().clone()) as ArrayRef,
                            (0..len).map(|i| ma.is_valid(i)).collect(),
                        )
                    } else if let Some(la) = array
                        .as_any()
                        .downcast_ref::<arrow::array::LargeListArray>()
                    {
                        (
                            la.value_offsets().to_vec(),
                            la.values().clone(),
                            (0..len).map(|i| la.is_valid(i)).collect(),
                        )
                    } else {
                        return Err(FederationError::invalid_input("expected list or map array"));
                    };
                let offset = |o: i64| {
                    usize::try_from(o).map_err(|_| {
                        FederationError::invalid_input(format!("invalid list offset {o}"))
                    })
                };
                *slots = (0..len)
                    .map(|i| {
                        Ok(if nulls[i] {
                            Some((offset(offsets[i])?, offset(offsets[i + 1])?))
                        } else {
                            None
                        })
                    })
                    .collect::<Result<_>>()?;
                child.load_array(values.as_ref())?;
            }
        }
        self.set_value_count(len);
        Ok(())
    }
}

fn integral_bits(value: &Value) -> Option<i64> {
    match value {
        Value::Byte(v) => Some(i64::from(*v)),
        Value::Short(v) => Some(i64::from(*v)),
        Value::Char(v) => Some(i64::from(*v)),
        Value::Int(v) => Some(i64::from(*v)),
        Value::Long(v) => Some(*v),
        _ => None,
    }
}

fn list_overflow() -> FederationError {
    FederationError::illegal_state("list vector exceeds the maximum Arrow offset")
}

fn checked_decimal(data_type: &DataType, d: &BigDecimal) -> Result<BigDecimal> {
    let (precision, scale) = super::types::decimal_precision_scale(data_type).unwrap_or((38, 0));
    let (_, value_scale) = d.as_bigint_and_exponent();
    if value_scale != i64::from(scale) {
        return Err(FederationError::UnsupportedOperation(format!(
            "BigDecimal scale must equal that in the Arrow vector: {value_scale} != {scale}"
        )));
    }
    if decimal_precision(d) > u64::from(precision) {
        return Err(FederationError::UnsupportedOperation(format!(
            "BigDecimal precision cannot be greater than that in the Arrow vector: {} > {}",
            decimal_precision(d),
            precision
        )));
    }
    Ok(d.clone())
}

fn opt_of<T>(values: &[Value], len: usize, f: impl Fn(&Value) -> Option<T>) -> Vec<Option<T>> {
    (0..len).map(|i| values.get(i).and_then(&f)).collect()
}

fn int_or_long(v: &Value) -> Option<i64> {
    match v {
        Value::Long(v) => Some(*v),
        Value::Int(v) => Some(i64::from(*v)),
        _ => None,
    }
}

fn opt_i64(values: &[Value], len: usize) -> Vec<Option<i64>> {
    opt_of(values, len, int_or_long)
}

fn opt_i32(values: &[Value], len: usize) -> Vec<Option<i32>> {
    opt_of(values, len, Value::as_i32)
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

fn decimal128_array(data_type: &DataType, values: &[Value], len: usize) -> Result<ArrayRef> {
    let (p, s) = super::types::decimal_precision_scale(data_type).unwrap_or((38, 0));
    let v = opt_of(values, len, |v| match v {
        Value::Decimal(d) => big_decimal_to_unscaled(d, s).to_i128(),
        _ => None,
    });
    Ok(Arc::new(
        Decimal128Array::from(v).with_precision_and_scale(p, s)?,
    ))
}

fn decimal256_array(data_type: &DataType, values: &[Value], len: usize) -> Result<ArrayRef> {
    let (p, s) = super::types::decimal_precision_scale(data_type).unwrap_or((76, 0));
    let v = opt_of(values, len, |v| match v {
        Value::Decimal(d) => {
            let bytes = big_decimal_to_unscaled(d, s).to_signed_bytes_le();
            let mut buf = if bytes.last().is_some_and(|b| b & 0x80 != 0) {
                [0xffu8; 32]
            } else {
                [0u8; 32]
            };
            let n = bytes.len().min(32);
            buf[..n].copy_from_slice(&bytes[..n]);
            Some(i256::from_le_bytes(buf))
        }
        _ => None,
    });
    Ok(Arc::new(
        Decimal256Array::from(v).with_precision_and_scale(p, s)?,
    ))
}

fn scalar_to_array(
    field: &FieldRef,
    minor: MinorType,
    values: &[Value],
    len: usize,
) -> Result<ArrayRef> {
    let get = |i: usize| values.get(i).unwrap_or(&Value::Null);
    let arr: ArrayRef = match minor {
        MinorType::NULL => Arc::new(NullArray::new(len)),
        MinorType::BIT => Arc::new(BooleanArray::from(
            (0..len).map(|i| get(i).as_bool()).collect::<Vec<_>>(),
        )),
        MinorType::TINYINT => Arc::new(Int8Array::from(opt_of(values, len, Value::as_i8))),
        MinorType::UINT1 => Arc::new(UInt8Array::from(opt_of(values, len, |v| {
            v.as_i8().map(i8::cast_unsigned)
        }))),
        MinorType::SMALLINT => Arc::new(Int16Array::from(opt_of(values, len, Value::as_i16))),
        MinorType::UINT2 => Arc::new(UInt16Array::from(opt_of(values, len, Value::as_u16))),
        MinorType::INT => Arc::new(Int32Array::from(opt_i32(values, len))),
        MinorType::UINT4 => Arc::new(UInt32Array::from(opt_of(values, len, |v| {
            v.as_i32().map(i32::cast_unsigned)
        }))),
        MinorType::BIGINT => Arc::new(Int64Array::from(opt_i64(values, len))),
        MinorType::UINT8 => Arc::new(UInt64Array::from(opt_of(values, len, |v| {
            int_or_long(v).map(i64::cast_unsigned)
        }))),
        MinorType::FLOAT4 => Arc::new(Float32Array::from(opt_of(values, len, Value::as_f32))),
        MinorType::FLOAT8 => Arc::new(Float64Array::from(opt_of(values, len, |v| match v {
            Value::Double(d) => Some(*d),
            _ => None,
        }))),
        MinorType::DATEDAY => Arc::new(Date32Array::from(opt_i32(values, len))),
        MinorType::DATEMILLI => Arc::new(Date64Array::from(opt_i64(values, len))),
        MinorType::TIMESEC => Arc::new(Time32SecondArray::from(opt_i32(values, len))),
        MinorType::TIMEMILLI => Arc::new(Time32MillisecondArray::from(opt_i32(values, len))),
        MinorType::TIMEMICRO => Arc::new(Time64MicrosecondArray::from(opt_i64(values, len))),
        MinorType::TIMENANO => Arc::new(Time64NanosecondArray::from(opt_i64(values, len))),
        MinorType::TIMESTAMPSEC
        | MinorType::TIMESTAMPMILLI
        | MinorType::TIMESTAMPMICRO
        | MinorType::TIMESTAMPNANO
        | MinorType::TIMESTAMPSECTZ
        | MinorType::TIMESTAMPMILLITZ
        | MinorType::TIMESTAMPMICROTZ
        | MinorType::TIMESTAMPNANOTZ => timestamp_array(field.data_type(), opt_i64(values, len)),
        MinorType::DECIMAL => decimal128_array(field.data_type(), values, len)?,
        MinorType::DECIMAL256 => decimal256_array(field.data_type(), values, len)?,
        MinorType::VARCHAR => Arc::new(StringArray::from(
            (0..len)
                .map(|i| get(i).as_str().map(std::string::ToString::to_string))
                .collect::<Vec<_>>(),
        )),
        MinorType::LARGEVARCHAR => Arc::new(LargeStringArray::from(
            (0..len)
                .map(|i| get(i).as_str().map(std::string::ToString::to_string))
                .collect::<Vec<_>>(),
        )),
        MinorType::VARBINARY => Arc::new(BinaryArray::from(
            (0..len).map(|i| get(i).as_bytes()).collect::<Vec<_>>(),
        )),
        MinorType::LARGEVARBINARY => Arc::new(LargeBinaryArray::from(
            (0..len).map(|i| get(i).as_bytes()).collect::<Vec<_>>(),
        )),
        MinorType::FIXEDSIZEBINARY => {
            let size = match field.data_type() {
                DataType::FixedSizeBinary(n) => *n,
                _ => 0,
            };
            if len == 0 || (0..len).all(|i| get(i).is_null()) {
                new_null_array(field.data_type(), len)
            } else {
                Arc::new(FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    (0..len).map(|i| get(i).as_bytes().map(<[u8]>::to_vec)),
                    size,
                )?)
            }
        }
        other => {
            return Err(FederationError::not_supported(format!(
                "Unsupported type {} for field {}",
                other,
                field.name()
            )))
        }
    };
    Ok(arr)
}

fn downcast<'a, T: 'static>(array: &'a dyn Array, message: Option<&str>) -> Result<&'a T> {
    array.as_any().downcast_ref::<T>().ok_or_else(|| {
        FederationError::invalid_input(message.map_or_else(
            || format!("unexpected array type {:?}", array.data_type()),
            str::to_string,
        ))
    })
}

fn collect_values<A: ArrayAccessor + Copy>(a: A, f: impl Fn(A::Item) -> Value) -> Vec<Value> {
    (0..a.len())
        .map(|i| {
            if a.is_valid(i) {
                f(a.value(i))
            } else {
                Value::Null
            }
        })
        .collect()
}

macro_rules! collect_prim {
    ($array:expr, $t:ty, $f:expr) => {
        collect_values(downcast::<$t>($array, None)?, $f)
    };
}

fn array_to_values(minor: MinorType, array: &dyn Array) -> Result<Vec<Value>> {
    Ok(match minor {
        MinorType::NULL => vec![Value::Null; array.len()],
        MinorType::BIT => collect_prim!(array, BooleanArray, Value::Boolean),
        MinorType::TINYINT => collect_prim!(array, Int8Array, Value::Byte),
        MinorType::UINT1 => collect_prim!(array, UInt8Array, |v: u8| Value::Byte(v.cast_signed())),
        MinorType::SMALLINT => collect_prim!(array, Int16Array, Value::Short),
        MinorType::UINT2 => collect_prim!(array, UInt16Array, Value::Char),
        MinorType::INT => collect_prim!(array, Int32Array, Value::Int),
        MinorType::UINT4 => collect_prim!(array, UInt32Array, |v: u32| Value::Int(v.cast_signed())),
        MinorType::BIGINT => collect_prim!(array, Int64Array, Value::Long),
        MinorType::UINT8 => {
            collect_prim!(array, UInt64Array, |v: u64| Value::Long(v.cast_signed()))
        }
        MinorType::FLOAT4 => collect_prim!(array, Float32Array, Value::Float),
        MinorType::FLOAT8 => collect_prim!(array, Float64Array, Value::Double),
        MinorType::DATEDAY => collect_prim!(array, Date32Array, Value::Int),
        MinorType::DATEMILLI => collect_prim!(array, Date64Array, Value::Long),
        MinorType::TIMESEC => collect_prim!(array, Time32SecondArray, Value::Int),
        MinorType::TIMEMILLI => collect_prim!(array, Time32MillisecondArray, Value::Int),
        MinorType::TIMEMICRO => collect_prim!(array, Time64MicrosecondArray, Value::Long),
        MinorType::TIMENANO => collect_prim!(array, Time64NanosecondArray, Value::Long),
        MinorType::TIMESTAMPSEC | MinorType::TIMESTAMPSECTZ => {
            collect_prim!(array, TimestampSecondArray, Value::Long)
        }
        MinorType::TIMESTAMPMILLI | MinorType::TIMESTAMPMILLITZ => {
            collect_prim!(array, TimestampMillisecondArray, Value::Long)
        }
        MinorType::TIMESTAMPMICRO | MinorType::TIMESTAMPMICROTZ => {
            collect_prim!(array, TimestampMicrosecondArray, Value::Long)
        }
        MinorType::TIMESTAMPNANO | MinorType::TIMESTAMPNANOTZ => {
            collect_prim!(array, TimestampNanosecondArray, Value::Long)
        }
        MinorType::DECIMAL => {
            let a = downcast::<Decimal128Array>(array, Some("expected decimal128 array"))?;
            let scale = i64::from(a.scale());
            collect_values(a, |v| {
                Value::Decimal(BigDecimal::new(BigInt::from(v), scale))
            })
        }
        MinorType::DECIMAL256 => {
            let a = downcast::<Decimal256Array>(array, Some("expected decimal256 array"))?;
            let scale = i64::from(a.scale());
            collect_values(a, |v| {
                let unscaled = BigInt::from_signed_bytes_le(&v.to_le_bytes());
                Value::Decimal(BigDecimal::new(unscaled, scale))
            })
        }
        MinorType::VARCHAR => {
            collect_prim!(array, StringArray, |v: &str| Value::String(v.to_string()))
        }
        MinorType::LARGEVARCHAR => collect_prim!(array, LargeStringArray, |v: &str| Value::String(
            v.to_string()
        )),
        MinorType::VIEWVARCHAR => {
            collect_prim!(array, StringViewArray, |v: &str| Value::String(
                v.to_string()
            ))
        }
        MinorType::VARBINARY => {
            collect_prim!(array, BinaryArray, |v: &[u8]| Value::Bytes(v.to_vec()))
        }
        MinorType::LARGEVARBINARY => {
            collect_prim!(array, LargeBinaryArray, |v: &[u8]| Value::Bytes(v.to_vec()))
        }
        MinorType::FIXEDSIZEBINARY => {
            collect_prim!(array, FixedSizeBinaryArray, |v: &[u8]| Value::Bytes(
                v.to_vec()
            ))
        }
        other => {
            return Err(FederationError::not_supported(format!(
                "Unsupported type {other}"
            )));
        }
    })
}

#[derive(Debug, Clone, Copy, Default)]
struct ListCursor {
    initialized: bool,
    for_pos: usize,
    current: usize,
    end: usize,
}

#[derive(Debug, Clone)]
pub struct FieldReader<'a> {
    vector: &'a FieldVector,
    pos: Rc<Cell<usize>>,
    cursor: Rc<Cell<ListCursor>>,
    child_pos: Rc<Cell<usize>>,
}

impl<'a> FieldReader<'a> {
    pub fn new(vector: &'a FieldVector) -> FieldReader<'a> {
        FieldReader::with_position(vector, Rc::new(Cell::new(0)))
    }

    fn with_position(vector: &'a FieldVector, pos: Rc<Cell<usize>>) -> FieldReader<'a> {
        FieldReader {
            vector,
            pos,
            cursor: Rc::new(Cell::new(ListCursor::default())),
            child_pos: Rc::new(Cell::new(0)),
        }
    }

    pub fn vector(&self) -> &'a FieldVector {
        self.vector
    }

    pub fn field(&self) -> &'a FieldRef {
        self.vector.field()
    }

    pub fn minor_type(&self) -> MinorType {
        self.vector.minor_type()
    }

    pub fn set_position(&self, index: usize) {
        self.pos.set(index);
        self.reset_cursor();
    }

    pub fn position(&self) -> usize {
        self.pos.get()
    }

    fn reset_cursor(&self) {
        let p = self.pos.get();
        let (start, end) = self.vector.list_slot(p).unwrap_or((0, 0));
        self.cursor.set(ListCursor {
            initialized: true,
            for_pos: p,
            current: start,
            end,
        });
    }

    pub fn is_set(&self) -> bool {
        self.vector.is_set(self.pos.get())
    }

    pub fn read_object(&self) -> Value {
        self.vector.get_object(self.pos.get())
    }

    pub fn next(&self) -> bool {
        let c = self.cursor.get();
        if !c.initialized || c.for_pos != self.pos.get() {
            self.reset_cursor();
        }
        let mut c = self.cursor.get();
        if c.current < c.end {
            self.child_pos.set(c.current);
            c.current += 1;
            self.cursor.set(c);
            true
        } else {
            false
        }
    }

    pub fn size(&self) -> usize {
        self.vector
            .list_slot(self.pos.get())
            .map_or(0, |(s, e)| e - s)
    }

    pub fn reader(&self) -> Option<FieldReader<'a>> {
        self.vector
            .data_vector()
            .map(|child| FieldReader::with_position(child, self.child_pos.clone()))
    }

    pub fn reader_named(&self, name: &str) -> Option<FieldReader<'a>> {
        self.vector
            .get_child(name)
            .map(|child| FieldReader::with_position(child, self.pos.clone()))
    }

    pub fn key(&self) -> Option<FieldReader<'a>> {
        self.reader()
            .and_then(|entries| entries.reader_named("key"))
    }

    pub fn value(&self) -> Option<FieldReader<'a>> {
        self.reader()
            .and_then(|entries| entries.reader_named("value"))
    }

    pub fn read_boolean(&self) -> Option<bool> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Boolean(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_byte(&self) -> Option<i8> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Byte(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_short(&self) -> Option<i16> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Short(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_character(&self) -> Option<u16> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Char(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_integer(&self) -> Option<i32> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Int(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_long(&self) -> Option<i64> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Long(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_float(&self) -> Option<f32> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Float(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_double(&self) -> Option<f64> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Double(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_big_decimal(&self) -> Option<BigDecimal> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Decimal(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_text(&self) -> Option<String> {
        match self.vector.get_raw(self.pos.get()) {
            Value::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_byte_array(&self) -> Option<Vec<u8>> {
        match self.vector.get_raw(self.pos.get()) {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_local_date_time(&self) -> Option<NaiveDateTime> {
        self.vector.get_local_date_time(self.pos.get())
    }
}
