use std::fmt;
use std::sync::Arc;

use arrow::datatypes::{DataType, SchemaRef};
use bigdecimal::{BigDecimal, RoundingMode};
use indexmap::IndexMap;

use crate::data::block::Block;
use crate::data::java_time::local_date_time_of_epoch_milli_utc;
use crate::data::types::{decimal_precision_scale, MinorType};
use crate::data::value::Value;
use crate::data::vector::FieldVector;
use crate::domain::predicate::{ConstraintEvaluator, ConstraintProjector, Constraints};
use crate::exceptions::{FederationError, Result};

pub struct GeneratedRowWriter<C> {
    extractors: IndexMap<String, Extractor<C>>,
    field_writer_factories: IndexMap<String, FieldWriterFactory<C>>,
    constraints: IndexMap<String, ConstraintProjector>,
    constraint_evaluator: Option<Arc<ConstraintEvaluator>>,
    field_writers: Vec<Box<dyn FieldWriter<C>>>,
    schema: Option<SchemaRef>,
}

pub struct RowWriterBuilder<C> {
    constraint_evaluator: Option<Arc<ConstraintEvaluator>>,
    constraints: IndexMap<String, ConstraintProjector>,
    extractors: IndexMap<String, Extractor<C>>,
    field_writer_factories: IndexMap<String, FieldWriterFactory<C>>,
}

impl<C> RowWriterBuilder<C> {
    #[must_use]
    pub fn with_extractor(mut self, field_name: &str, extractor: Extractor<C>) -> Self {
        self.extractors.insert(field_name.to_string(), extractor);
        self
    }

    #[must_use]
    pub fn with_field_writer_factory(
        mut self,
        field_name: &str,
        factory: FieldWriterFactory<C>,
    ) -> Self {
        self.field_writer_factories
            .insert(field_name.to_string(), factory);
        self
    }

    #[must_use]
    pub fn with_constraint(mut self, field_name: &str, constraint: ConstraintProjector) -> Self {
        self.constraints.insert(field_name.to_string(), constraint);
        self
    }

    pub fn build(self) -> GeneratedRowWriter<C> {
        GeneratedRowWriter {
            extractors: self.extractors,
            field_writer_factories: self.field_writer_factories,
            constraints: self.constraints,
            constraint_evaluator: self.constraint_evaluator,
            field_writers: Vec::new(),
            schema: None,
        }
    }
}

impl<C: 'static> GeneratedRowWriter<C> {
    pub fn new_builder() -> RowWriterBuilder<C> {
        RowWriterBuilder {
            constraint_evaluator: None,
            constraints: IndexMap::new(),
            extractors: IndexMap::new(),
            field_writer_factories: IndexMap::new(),
        }
    }

    pub fn new_builder_with_constraints(constraints: &Constraints) -> RowWriterBuilder<C> {
        let mut builder = GeneratedRowWriter::new_builder();
        for (name, value_set) in constraints.summary() {
            let value_set = value_set.clone();
            let projector: ConstraintProjector =
                Arc::new(move |value: &crate::data::Value| value_set.contains_value(value));
            builder.constraints.insert(name.clone(), projector);
        }
        builder
    }

    pub fn new_builder_with_evaluator(
        constraint_evaluator: Arc<ConstraintEvaluator>,
    ) -> RowWriterBuilder<C> {
        let mut builder = GeneratedRowWriter::new_builder();
        builder.constraint_evaluator = Some(constraint_evaluator);
        builder
    }

    pub fn write_row(&mut self, block: &mut Block, row_num: usize, context: &C) -> Result<bool> {
        self.check_and_recompile(block)?;
        let mut matched = true;
        for (writer, vector) in self
            .field_writers
            .iter_mut()
            .zip(block.field_vectors_mut().iter_mut())
        {
            matched &= writer.write(vector, context, row_num)?;
        }
        Ok(matched)
    }

    fn check_and_recompile(&mut self, block: &Block) -> Result<()> {
        let same = match &self.schema {
            Some(s) => {
                Arc::ptr_eq(s, block.schema())
                    && self.field_writers.len() == block.field_vectors().len()
            }
            None => false,
        };
        if !same {
            tracing::info!("recompile: Detected a new block, rebuilding field writers so they point to the correct Arrow vectors.");
            self.schema = Some(block.schema().clone());
            self.field_writers.clear();
            let mut writers = Vec::with_capacity(block.field_vectors().len());
            for vector in block.field_vectors() {
                writers.push(self.make_field_writer(vector)?);
            }
            self.field_writers = writers;
        }
        Ok(())
    }

    fn constraint_for(&self, field_name: &str) -> Option<ConstraintProjector> {
        if let Some(c) = self.constraints.get(field_name) {
            return Some(c.clone());
        }
        self.constraint_evaluator
            .as_ref()
            .and_then(|e| e.make_constraint_projector(field_name))
    }

    fn make_field_writer(&self, vector: &FieldVector) -> Result<Box<dyn FieldWriter<C>>> {
        let field_name = vector.name().to_string();
        let field_type = vector.minor_type();
        let extractor = self.extractors.get(&field_name);
        let constraint = self.constraint_for(&field_name);
        if let Some(factory) = self.field_writer_factories.get(&field_name) {
            return factory(vector, extractor, constraint);
        }
        let extractor = extractor.ok_or_else(|| {
            FederationError::invalid_input(format!("Missing extractor for field[{field_name}]"))
        })?;
        match field_type {
            _ if extractor.minor_type() == field_type => Ok(Box::new(ScalarFieldWriter::new(
                extractor.clone(),
                constraint,
            ))),
            MinorType::Int
            | MinorType::BigInt
            | MinorType::DateMilli
            | MinorType::DateDay
            | MinorType::TinyInt
            | MinorType::SmallInt
            | MinorType::Float4
            | MinorType::Float8
            | MinorType::Decimal
            | MinorType::Bit
            | MinorType::VarChar
            | MinorType::VarBinary => Err(FederationError::ClassCast(format!(
                "{} cannot be used for field {} of type {}",
                extractor.type_name(),
                field_name,
                field_type
            ))),
            other => Err(FederationError::invalid_input(format!(
                "{other} is not supported"
            ))),
        }
    }
}

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

pub trait FieldWriter<C>: Send {
    fn write(&mut self, vector: &mut FieldVector, context: &C, row_num: usize) -> Result<bool>;
}

pub type FieldWriterFactory<C> = Arc<
    dyn Fn(
            &FieldVector,
            Option<&Extractor<C>>,
            Option<ConstraintProjector>,
        ) -> Result<Box<dyn FieldWriter<C>>>
        + Send
        + Sync,
>;

pub struct ScalarFieldWriter<C> {
    extractor: Extractor<C>,
    constraint: Option<ConstraintProjector>,
}

impl<C> ScalarFieldWriter<C> {
    pub fn new(
        extractor: Extractor<C>,
        constraint: Option<ConstraintProjector>,
    ) -> ScalarFieldWriter<C> {
        ScalarFieldWriter {
            extractor,
            constraint,
        }
    }
}

impl<C> FieldWriter<C> for ScalarFieldWriter<C> {
    fn write(&mut self, vector: &mut FieldVector, context: &C, row_num: usize) -> Result<bool> {
        let extracted = self
            .extractor
            .extract_values(context, vector.field().data_type())?;
        let constraint_value = if let Some((stored, seen)) = extracted {
            vector.set(row_num, &stored)?;
            seen
        } else {
            vector.set_null(row_num);
            Value::Null
        };
        self.constraint
            .as_ref()
            .map_or(Ok(true), |constraint| constraint(&constraint_value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::block_allocator::{BlockAllocator, BlockAllocatorImpl};
    use crate::data::schema_builder::SchemaBuilder;
    use crate::data::value::Value;
    use arrow::datatypes::DataType;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Row {
        id: i32,
        name: Option<String>,
    }

    #[test]
    fn write_row_uses_extractors_and_constraints() {
        let schema = SchemaBuilder::new()
            .add_int_field("id")
            .add_string_field("name")
            .build();
        let allocator = BlockAllocatorImpl::new();
        let mut block = allocator.create_block(Arc::new(schema));
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_inner = calls.clone();
        let mut writer = GeneratedRowWriter::new_builder()
            .with_extractor("id", Extractor::Int(Arc::new(|row: &Row| Ok(Some(row.id)))))
            .with_extractor(
                "name",
                Extractor::VarChar(Arc::new(|row: &Row| Ok(row.name.clone()))),
            )
            .with_constraint(
                "id",
                Arc::new(move |v: &Value| {
                    calls_inner.fetch_add(1, Ordering::SeqCst);
                    Ok(matches!(v, Value::Int(x) if *x > 1))
                }),
            )
            .build();
        let rows = [
            Row {
                id: 1,
                name: Some("a".into()),
            },
            Row { id: 2, name: None },
        ];
        let r0 = writer.write_row(&mut block, 0, &rows[0]).unwrap();
        let r1 = writer.write_row(&mut block, 1, &rows[1]).unwrap();
        block.set_row_count(2);
        assert!(!r0);
        assert!(r1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let reader = block.get_field_reader("name").unwrap();
        let reader = reader.at(0);
        assert_eq!(reader.read_text(), Some("a".to_string()));
        let reader = reader.at(1);
        assert!(!reader.is_set());
        let ids = block.get_field_reader("id").unwrap();
        let ids = ids.at(1);
        assert_eq!(ids.read_integer(), Some(2));
    }

    #[test]
    fn missing_extractor_is_an_error() {
        let schema = SchemaBuilder::new()
            .add_field_with_type("x", DataType::Int32)
            .build();
        let allocator = BlockAllocatorImpl::new();
        let mut block = allocator.create_block(Arc::new(schema));
        let mut writer = GeneratedRowWriter::<()>::new_builder().build();
        let err = writer.write_row(&mut block, 0, &()).unwrap_err();
        assert!(err.to_string().contains("Missing extractor for field[x]"));
    }
}
