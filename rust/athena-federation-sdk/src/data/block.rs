use std::fmt;
use std::sync::Arc;

use arrow::array::ArrayRef;
use arrow::datatypes::{Field, SchemaRef};
use arrow::record_batch::{RecordBatch, RecordBatchOptions};

use crate::data::arrow_type_comparator::ArrowTypeComparator;
use crate::data::block_utils;
use crate::data::date_time_formatter_util::TzEncoding;
use crate::data::record_batch_serde::ArrowRecordBatch;
use crate::data::types::{minor_type_of, MinorType};
use crate::data::value::Value;
use crate::data::vector::{FieldReader, FieldVector};
use crate::domain::predicate::ConstraintEvaluator;
use crate::exceptions::{FederationError, Result};

#[derive(Clone)]
pub struct Block {
    allocator_id: String,
    schema: SchemaRef,
    vectors: Vec<FieldVector>,
    row_count: usize,
    constraint_evaluator: Arc<ConstraintEvaluator>,
}

impl Block {
    pub fn new(allocator_id: impl Into<String>, schema: impl Into<SchemaRef>) -> Block {
        let schema: SchemaRef = schema.into();
        let vectors = schema
            .fields()
            .iter()
            .map(|f| FieldVector::new(f.clone()))
            .collect();
        Block {
            allocator_id: allocator_id.into(),
            schema,
            vectors,
            row_count: 0,
            constraint_evaluator: Arc::new(ConstraintEvaluator::empty_evaluator()),
        }
    }

    pub fn set_tz_encoding(&mut self, encoding: TzEncoding) {
        for vector in &mut self.vectors {
            vector.set_tz_encoding(encoding);
        }
    }

    pub fn constrain(&mut self, constraint_evaluator: Option<Arc<ConstraintEvaluator>>) {
        self.constraint_evaluator = constraint_evaluator
            .unwrap_or_else(|| Arc::new(ConstraintEvaluator::empty_evaluator()));
    }

    pub fn constraint_evaluator(&self) -> &Arc<ConstraintEvaluator> {
        &self.constraint_evaluator
    }

    pub fn allocator_id(&self) -> &str {
        &self.allocator_id
    }

    pub fn schema(&self) -> &SchemaRef {
        &self.schema
    }

    pub fn get_meta_data(&self, key: &str) -> Option<&str> {
        self.schema.metadata().get(key).map(String::as_str)
    }

    pub fn set_value(
        &mut self,
        field_name: &str,
        row: usize,
        value: impl Into<Value>,
    ) -> Result<bool> {
        let value = value.into();
        if self.constraint_evaluator.apply(field_name, &value)? {
            let vector = self.get_field_vector_mut(field_name).ok_or_else(|| {
                FederationError::null_pointer(format!("No vector found for field {field_name}"))
            })?;
            block_utils::set_value(vector, row, &value)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn offer_value(
        &mut self,
        field_name: &str,
        row: usize,
        value: impl Into<Value>,
    ) -> Result<bool> {
        let value = value.into();
        if self.constraint_evaluator.apply(field_name, &value)? {
            if let Some(vector) = self.get_field_vector_mut(field_name) {
                block_utils::set_value(vector, row, &value)?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub fn set_complex_value(
        &mut self,
        field_name: &str,
        row: usize,
        field_resolver: &dyn FieldResolver,
        value: impl Into<Value>,
    ) -> Result<bool> {
        let value = value.into();
        let vector = self.get_field_vector_mut(field_name).ok_or_else(|| {
            FederationError::null_pointer(format!("No vector found for field {field_name}"))
        })?;
        block_utils::set_complex_value(vector, row, field_resolver, &value)?;
        Ok(true)
    }

    pub fn offer_complex_value(
        &mut self,
        field_name: &str,
        row: usize,
        field_resolver: &dyn FieldResolver,
        value: impl Into<Value>,
    ) -> Result<bool> {
        let value = value.into();
        if let Some(vector) = self.get_field_vector_mut(field_name) {
            block_utils::set_complex_value(vector, row, field_resolver, &value)?;
        }
        Ok(true)
    }

    pub fn set_row_count(&mut self, row_count: usize) {
        self.row_count = row_count;
        for v in &mut self.vectors {
            v.set_value_count(row_count);
        }
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn get_field_reader(&self, field_name: &str) -> Option<FieldReader<'_>> {
        self.get_field_vector(field_name).map(FieldReader::new)
    }

    pub fn get_field_vector(&self, field_name: &str) -> Option<&FieldVector> {
        self.vectors.iter().find(|v| v.name() == field_name)
    }

    pub fn get_field_vector_mut(&mut self, field_name: &str) -> Option<&mut FieldVector> {
        self.vectors.iter_mut().find(|v| v.name() == field_name)
    }

    pub fn field_readers(&self) -> Vec<FieldReader<'_>> {
        self.vectors.iter().map(FieldReader::new).collect()
    }

    pub fn get_size(&self) -> usize {
        self.vectors
            .iter()
            .map(super::vector::FieldVector::get_buffer_size)
            .sum()
    }

    pub fn field_vectors(&self) -> &[FieldVector] {
        &self.vectors
    }

    pub fn field_vectors_mut(&mut self) -> &mut [FieldVector] {
        &mut self.vectors
    }

    pub fn get_record_batch(&self) -> Result<RecordBatch> {
        let arrays = self
            .vectors
            .iter()
            .map(|v| v.to_array_len(self.row_count))
            .collect::<Result<Vec<ArrayRef>>>()?;
        let options = RecordBatchOptions::new().with_row_count(Some(self.row_count));
        Ok(RecordBatch::try_new_with_options(
            self.schema.clone(),
            arrays,
            &options,
        )?)
    }

    pub fn get_arrow_record_batch(&self) -> Result<ArrowRecordBatch> {
        ArrowRecordBatch::from_record_batch(&self.get_record_batch()?)
    }

    pub fn load_record_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        if batch.num_columns() != self.vectors.len() {
            return Err(FederationError::illegal_argument(format!(
                "Batch has {} columns but block has {}",
                batch.num_columns(),
                self.vectors.len()
            )));
        }
        for (vector, column) in self.vectors.iter_mut().zip(batch.columns()) {
            vector.reset();
            vector.load_array(column.as_ref())?;
        }
        self.set_row_count(batch.num_rows());
        Ok(())
    }

    pub fn load_arrow_record_batch(&mut self, batch: &ArrowRecordBatch) -> Result<()> {
        let decoded = batch.decode(self.schema.clone())?;
        self.load_record_batch(&decoded)
    }

    pub fn equals_as_set(&self, that: &Block) -> bool {
        if self.schema.fields().len() != that.schema.fields().len() {
            return false;
        }
        if self.row_count != that.row_count {
            return false;
        }
        for next in self.schema.fields() {
            let Some(this_reader) = self.get_field_reader(next.name()) else {
                return false;
            };
            let Some(that_reader) = that.get_field_reader(next.name()) else {
                return false;
            };
            for i in 0..self.row_count {
                let this_reader = this_reader.at(i);
                let val = this_reader.read_object();
                let mut matched = false;
                for j in 0..that.row_count {
                    let that_reader = that_reader.at(j);
                    match ArrowTypeComparator::compare_reader(
                        &that_reader,
                        &val,
                        &that_reader.read_object(),
                    ) {
                        Ok(0) => matched = true,
                        Ok(_) => {}
                        Err(_) => return false,
                    }
                }
                if !matched {
                    return false;
                }
            }
        }
        true
    }

    pub fn try_equals(&self, that: &Block) -> Result<bool> {
        if self.schema.fields().len() != that.schema.fields().len() {
            return Ok(false);
        }
        if self.row_count != that.row_count {
            return Ok(false);
        }
        for next in self.schema.fields() {
            let Some(this_reader) = self.get_field_reader(next.name()) else {
                return Ok(false);
            };
            let Some(that_reader) = that.get_field_reader(next.name()) else {
                return Ok(false);
            };
            for i in 0..self.row_count {
                let this_reader = this_reader.at(i);
                let that_reader = that_reader.at(i);
                match ArrowTypeComparator::compare_reader(
                    &this_reader,
                    &this_reader.read_object(),
                    &that_reader.read_object(),
                ) {
                    Ok(0) => {}
                    Ok(_) => return Ok(false),
                    Err(e @ FederationError::IllegalArgument(_)) => return Err(e),
                    Err(e) => {
                        tracing::warn!("equals: {}", e);
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.try_equals(other).unwrap_or(false)
    }
}

impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Block{{rows={}", self.row_count)?;
        let rows_to_print = self.row_count.min(10);
        for next in self.schema.fields() {
            let mut values = Vec::new();
            if let Some(reader) = self.get_field_reader(next.name()) {
                for i in 0..rows_to_print {
                    let reader = reader.at(i);
                    values.push(
                        block_utils::field_to_string(&reader).unwrap_or_else(|e| e.to_string()),
                    );
                }
            }
            write!(f, ", {}=[{}]", next.name(), values.join(", "))?;
        }
        write!(f, "}}")
    }
}

pub trait FieldResolver {
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value>;

    fn get_map_key(&self, _field: &Field, value: &Value) -> Result<Value> {
        Ok(value.clone())
    }

    fn get_map_value(&self, _field: &Field, value: &Value) -> Result<Value> {
        Ok(value.clone())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultFieldResolver;

impl FieldResolver for DefaultFieldResolver {
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value> {
        let minor_type = minor_type_of(field.data_type());
        if let Value::Map(_) = value {
            return Ok(value.map_get(field.name()).cloned().unwrap_or(Value::Null));
        } else if minor_type == MinorType::List {
            if let Value::List(_) = value {
                return Ok(value.clone());
            }
            return Err(FederationError::class_cast(
                value.java_class_name(),
                "java.util.List",
            ));
        }
        Err(FederationError::invalid_input(format!(
            "Expected LIST type but found {minor_type}"
        )))
    }
}

impl<F> FieldResolver for F
where
    F: Fn(&Field, &Value) -> Result<Value>,
{
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value> {
        self(field, value)
    }
}
