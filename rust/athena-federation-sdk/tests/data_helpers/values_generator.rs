use std::collections::HashSet;

use athena_federation_sdk::data::{decimal_precision_scale, MinorType};
use athena_federation_sdk::data::{FieldVector, Value};
use athena_federation_sdk::Result;
use bigdecimal::BigDecimal;
use num_bigint::BigInt;

use super::custom_field_vector::CustomFieldVector;
use super::rng::Rng;

pub const MIN_TIME: i64 = -8_000_000_000_000_000;
pub const MAX_TIME: i64 = 8_000_000_000_000_000;
const MIN_DATE_DAY: i64 = -25_567;
const MAX_DATE_DAY: i64 = 193_022;

pub struct ValuesGenerator {
    rng: Rng,
}

impl ValuesGenerator {
    pub fn new(seed: u64) -> ValuesGenerator {
        ValuesGenerator {
            rng: Rng::new(seed),
        }
    }

    pub fn generate_values_root(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
    ) -> Result<()> {
        self.generate_values(vector, custom, 0, false)?;
        Ok(())
    }

    fn get_length(&mut self, length: usize) -> usize {
        if length == 0 {
            usize::try_from(self.rng.between(1, 5)).unwrap()
        } else {
            length
        }
    }

    fn is_null(&mut self, nullable: bool) -> bool {
        nullable && self.rng.between(0, 5) == 0
    }

    pub fn generate_values(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
        length: usize,
        unique: bool,
    ) -> Result<usize> {
        match vector.minor_type() {
            MinorType::List => self.set_list(vector, custom, length),
            MinorType::Map => self.set_map(vector, custom, length),
            MinorType::Struct => self.set_struct(vector, custom, length),
            _ => self.set_primitive(vector, custom, length, unique),
        }
    }

    fn set_struct(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
        length: usize,
    ) -> Result<usize> {
        let position = vector.value_count();
        let names: Vec<String> = vector
            .children()
            .iter()
            .map(|c| c.name().to_string())
            .collect();
        let mut max_child_size = 0;
        for name in names {
            let child = vector.get_child_mut(&name).expect("struct child");
            let mut child_custom = CustomFieldVector::new(child.field().clone());
            let size = self.generate_values(child, &mut child_custom, length, false)?;
            max_child_size = max_child_size.max(size);
            custom.add(Some(child_custom));
        }
        for i in position..max_child_size {
            vector.set_index_defined(i);
        }
        vector.set_value_count(max_child_size);
        Ok(vector.value_count())
    }

    fn set_list(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
        length: usize,
    ) -> Result<usize> {
        assert_eq!(vector.children().len(), 1);
        let position = vector.value_count();
        let nullable = vector.field().is_nullable();
        let length = self.get_length(length);
        let mut prev_child_size = vector
            .data_vector()
            .map_or(0, athena_federation_sdk::data::FieldVector::value_count);
        for i in 0..length {
            let mut child_custom = None;
            if !self.is_null(nullable) {
                vector.start_new_value(position + i)?;
                let child = vector.data_vector_mut().expect("list child");
                let mut cc = CustomFieldVector::new(child.field().clone());
                let new_child_size = self.generate_values(child, &mut cc, 0, false)?;
                vector.end_value(position + i, new_child_size - prev_child_size)?;
                prev_child_size = new_child_size;
                child_custom = Some(cc);
            }
            custom.add(child_custom);
        }
        vector.set_value_count(position + length);
        Ok(vector.value_count())
    }

    fn set_map(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
        length: usize,
    ) -> Result<usize> {
        assert_eq!(vector.children().len(), 1);
        let position = vector.value_count();
        let length = self.get_length(length);
        for i in 0..length {
            vector.start_new_value(position + i)?;
            let entries = vector.data_vector_mut().expect("entries");
            assert_eq!(entries.children().len(), 2);
            let key_minor = entries.get_child("key").expect("key").minor_type();
            let mut kv_length = self.get_length(0);
            if key_minor == MinorType::VarBinary || key_minor == MinorType::Bit {
                kv_length = 2;
            }
            let keys = entries.get_child_mut("key").expect("key");
            let mut keys_custom = CustomFieldVector::new(keys.field().clone());
            self.generate_values(keys, &mut keys_custom, kv_length, true)?;
            let values = entries.get_child_mut("value").expect("value");
            let mut values_custom = CustomFieldVector::new(values.field().clone());
            self.generate_values(values, &mut values_custom, kv_length, false)?;
            custom.add_map(keys_custom, values_custom);
            vector.end_value(position + i, kv_length)?;
        }
        vector.set_value_count(position + length);
        let entries = vector.data_vector_mut().expect("entries");
        let key_count = entries.get_child("key").expect("key").value_count();
        for i in 0..key_count {
            entries.set_index_defined(i);
        }
        entries.set_value_count(key_count);
        Ok(vector.value_count())
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "takes the low bits of a random word, as the Java generator does"
    )]
    fn random_value(&mut self, vector: &FieldVector) -> Value {
        let field = vector.field().clone();
        match vector.minor_type() {
            MinorType::BigInt => Value::Long(self.rng.next_u64().cast_signed()),
            MinorType::Bit => Value::Int(i32::try_from(self.rng.between(0, 1)).unwrap()),
            MinorType::DateDay => {
                Value::Int(i32::try_from(self.rng.between(MIN_DATE_DAY, MAX_DATE_DAY)).unwrap())
            }
            MinorType::DateMilli
            | MinorType::TimestampMicroTz
            | MinorType::TimestampMilliTz
            | MinorType::TimestampNanoTz
            | MinorType::TimestampSecTz => Value::Long(self.rng.between(MIN_TIME, MAX_TIME)),
            MinorType::Decimal => {
                let (precision, scale) =
                    decimal_precision_scale(field.data_type()).unwrap_or((10, 0));
                let bound = 10i128.pow(u32::from(precision)) - 1;
                let unscaled = self.rng.between_i128(-bound, bound);
                Value::Decimal(BigDecimal::new(BigInt::from(unscaled), i64::from(scale)))
            }
            MinorType::Float4 => Value::Float(self.random_float()),
            MinorType::Float8 => Value::Double(f64::from(self.random_float())),
            MinorType::Int => Value::Int(self.rng.next_u64() as i32),
            MinorType::SmallInt => Value::Short(self.rng.next_u64() as i16),
            MinorType::TinyInt => Value::Byte(self.rng.next_u64() as i8),
            MinorType::UInt1 => {
                Value::Byte(i8::try_from(self.rng.between(0, i64::from(i8::MAX))).unwrap())
            }
            MinorType::UInt2 => {
                Value::Short(i16::try_from(self.rng.between(0, i64::from(i16::MAX))).unwrap())
            }
            MinorType::UInt4 => {
                Value::Int(i32::try_from(self.rng.between(0, i64::from(i32::MAX))).unwrap())
            }
            MinorType::UInt8 => Value::Long(self.rng.between(0, i64::MAX)),
            MinorType::VarBinary => Value::Bytes(self.rng.string(0).into_bytes()),
            MinorType::VarChar => Value::String(self.rng.string(1)),
            other => panic!("Not yet implemented for typeId {other}"),
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "takes the low bits of a random word, as the Java generator does"
    )]
    fn random_float(&mut self) -> f32 {
        loop {
            let f = f32::from_bits(self.rng.next_u64() as u32);
            if f.is_finite() {
                return f;
            }
        }
    }

    fn set_primitive(
        &mut self,
        vector: &mut FieldVector,
        custom: &mut CustomFieldVector,
        length: usize,
        unique: bool,
    ) -> Result<usize> {
        let position = vector.value_count();
        let length = self.get_length(length);
        let nullable = vector.field().is_nullable();
        let mut items = Vec::with_capacity(length);
        let mut seen = HashSet::new();
        while items.len() < length {
            let v = self.random_value(vector);
            if unique && !seen.insert(format!("{v:?}")) {
                continue;
            }
            items.push(v);
        }
        for (i, item) in items.iter().enumerate() {
            if self.is_null(nullable) {
                vector.set_null(i + position);
            } else {
                vector.set(i + position, item)?;
            }
            custom.add_value(vector.get_object(i + position));
        }
        vector.set_value_count(position + length);
        Ok(vector.value_count())
    }
}
