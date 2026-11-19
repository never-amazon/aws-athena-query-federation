use std::sync::Arc;

use arrow::array::{Array, ArrayRef, ListArray, MapArray, StructArray, UInt32Array};
use arrow::buffer::{NullBuffer, OffsetBuffer};
use arrow::datatypes::{DataType, FieldRef, Fields};
use bigdecimal::BigDecimal;
use chrono::NaiveDateTime;
use num_bigint::BigInt;

use super::column::Column;
use super::date_time_formatter_util::TzEncoding;
use super::java_time::{local_date_time_of_epoch_micro_utc, local_date_time_of_epoch_milli_utc};
use super::types::{minor_type_of, MinorType};
use super::value::Value;
use crate::exceptions::{FederationError, Result};
use crate::java_cast::{
    double_to_float, long_to_byte, long_to_char, long_to_double, long_to_float, long_to_int,
    long_to_short,
};

#[derive(Debug, Clone)]
enum VectorData {
    Scalar(Column),
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
    tz_encoding: TzEncoding,
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
            _ => VectorData::Scalar(Column::for_minor(minor_type)),
        };
        FieldVector {
            field,
            minor_type,
            value_count: 0,
            tz_encoding: TzEncoding::default(),
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
        let mut fresh = FieldVector::new(self.field.clone());
        fresh.set_tz_encoding(self.tz_encoding);
        *self = fresh;
    }

    pub fn tz_encoding(&self) -> TzEncoding {
        self.tz_encoding
    }

    pub fn set_tz_encoding(&mut self, encoding: TzEncoding) {
        self.tz_encoding = encoding;
        match &mut self.data {
            VectorData::Struct { children, .. } => {
                for c in children.iter_mut() {
                    c.set_tz_encoding(encoding);
                }
            }
            VectorData::List { child, .. } => child.set_tz_encoding(encoding),
            VectorData::Scalar(_) => {}
        }
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
            VectorData::Scalar(column) => column.is_set(index),
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
            VectorData::Scalar(column) => column.get(index),
            _ => self.get_object(index),
        }
    }

    pub fn get_object(&self, index: usize) -> Value {
        match &self.data {
            VectorData::Scalar(column) => {
                let raw = column.get(index);
                match (self.minor_type, &raw) {
                    (MinorType::DATEMILLI | MinorType::TIMESTAMPMILLI, Value::Long(ms)) => {
                        Value::LocalDateTime(local_date_time_of_epoch_milli_utc(*ms))
                    }
                    (MinorType::TIMESTAMPSEC, Value::Long(s)) => Value::LocalDateTime(
                        local_date_time_of_epoch_milli_utc(s.saturating_mul(1000)),
                    ),
                    (MinorType::TIMESTAMPMICRO, Value::Long(us)) => {
                        Value::LocalDateTime(local_date_time_of_epoch_micro_utc(*us))
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

    pub fn set_null(&mut self, index: usize) {
        match &mut self.data {
            VectorData::Scalar(column) => column.set_null(index),
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
            VectorData::Scalar(column) => column.len(),
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
        match &mut self.data {
            VectorData::Scalar(column) => column.put(index, canonical),
            _ => Err(FederationError::UnsupportedOperation(format!(
                "vector {} is not a scalar vector",
                self.field.name()
            ))),
        }
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
            VectorData::Scalar(column) => {
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
                let var = column.variable_width_bytes(n);
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
            VectorData::Scalar(column) => column.to_array(&self.field, self.minor_type, len),
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
            VectorData::Scalar(column) => {
                *column = Column::from_array(self.minor_type, array)?;
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

#[derive(Debug, Clone, Copy)]
pub struct FieldReader<'a> {
    vector: &'a FieldVector,
    pos: usize,
}

impl<'a> FieldReader<'a> {
    pub fn new(vector: &'a FieldVector) -> FieldReader<'a> {
        FieldReader { vector, pos: 0 }
    }

    #[must_use]
    pub fn at(self, pos: usize) -> FieldReader<'a> {
        FieldReader { pos, ..self }
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

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn elements(self) -> impl Iterator<Item = FieldReader<'a>> {
        let (start, end) = self.vector.list_slot(self.pos).unwrap_or((0, 0));
        let child = self.vector.data_vector();
        (start..end).filter_map(move |i| child.map(|c| FieldReader::new(c).at(i)))
    }

    pub fn is_set(&self) -> bool {
        self.vector.is_set(self.pos)
    }

    pub fn read_object(&self) -> Value {
        self.vector.get_object(self.pos)
    }

    pub fn size(&self) -> usize {
        self.vector.list_slot(self.pos).map_or(0, |(s, e)| e - s)
    }

    pub fn reader_named(&self, name: &str) -> Option<FieldReader<'a>> {
        self.vector
            .get_child(name)
            .map(|child| FieldReader::new(child).at(self.pos))
    }

    pub fn read_boolean(&self) -> Option<bool> {
        match self.vector.get_raw(self.pos) {
            Value::Boolean(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_byte(&self) -> Option<i8> {
        match self.vector.get_raw(self.pos) {
            Value::Byte(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_short(&self) -> Option<i16> {
        match self.vector.get_raw(self.pos) {
            Value::Short(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_character(&self) -> Option<u16> {
        match self.vector.get_raw(self.pos) {
            Value::Char(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_integer(&self) -> Option<i32> {
        match self.vector.get_raw(self.pos) {
            Value::Int(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_long(&self) -> Option<i64> {
        match self.vector.get_raw(self.pos) {
            Value::Long(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_float(&self) -> Option<f32> {
        match self.vector.get_raw(self.pos) {
            Value::Float(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_double(&self) -> Option<f64> {
        match self.vector.get_raw(self.pos) {
            Value::Double(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_big_decimal(&self) -> Option<BigDecimal> {
        match self.vector.get_raw(self.pos) {
            Value::Decimal(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_text(&self) -> Option<String> {
        match self.vector.get_raw(self.pos) {
            Value::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_byte_array(&self) -> Option<Vec<u8>> {
        match self.vector.get_raw(self.pos) {
            Value::Bytes(v) => Some(v),
            _ => None,
        }
    }

    pub fn read_local_date_time(&self) -> Option<NaiveDateTime> {
        self.vector.get_local_date_time(self.pos)
    }
}
