use std::any::Any;
use std::sync::Arc;

use arrow::datatypes::SchemaRef;
use indexmap::IndexMap;

use super::extractors::Extractor;
use super::fieldwriters::{
    BigIntFieldWriter, BitFieldWriter, DateDayFieldWriter, DateMilliFieldWriter,
    DecimalFieldWriter, FieldWriter, FieldWriterFactory, Float4FieldWriter, Float8FieldWriter,
    IntFieldWriter, SmallIntFieldWriter, TinyIntFieldWriter, VarBinaryFieldWriter,
    VarCharFieldWriter,
};
use crate::data::block::Block;
use crate::data::types::MinorType;
use crate::data::vector::FieldVector;
use crate::domain::predicate::{ConstraintEvaluator, ConstraintProjector, Constraints};
use crate::exceptions::{FederationError, Result};

pub struct GeneratedRowWriter {
    extractors: IndexMap<String, Extractor>,
    field_writer_factories: IndexMap<String, Arc<dyn FieldWriterFactory>>,
    constraints: IndexMap<String, Arc<dyn ConstraintProjector>>,
    constraint_evaluator: Option<Arc<ConstraintEvaluator>>,
    field_writers: Vec<Box<dyn FieldWriter>>,
    schema: Option<SchemaRef>,
}

pub struct RowWriterBuilder {
    constraint_evaluator: Option<Arc<ConstraintEvaluator>>,
    constraints: IndexMap<String, Arc<dyn ConstraintProjector>>,
    extractors: IndexMap<String, Extractor>,
    field_writer_factories: IndexMap<String, Arc<dyn FieldWriterFactory>>,
}

impl RowWriterBuilder {
    #[must_use]
    pub fn with_extractor(mut self, field_name: &str, extractor: Extractor) -> Self {
        self.extractors.insert(field_name.to_string(), extractor);
        self
    }

    #[must_use]
    pub fn with_field_writer_factory(
        mut self,
        field_name: &str,
        factory: Arc<dyn FieldWriterFactory>,
    ) -> Self {
        self.field_writer_factories
            .insert(field_name.to_string(), factory);
        self
    }

    #[must_use]
    pub fn with_constraint(
        mut self,
        field_name: &str,
        constraint: Arc<dyn ConstraintProjector>,
    ) -> Self {
        self.constraints.insert(field_name.to_string(), constraint);
        self
    }

    pub fn build(self) -> GeneratedRowWriter {
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

impl GeneratedRowWriter {
    pub fn new_builder() -> RowWriterBuilder {
        RowWriterBuilder {
            constraint_evaluator: None,
            constraints: IndexMap::new(),
            extractors: IndexMap::new(),
            field_writer_factories: IndexMap::new(),
        }
    }

    pub fn new_builder_with_constraints(constraints: &Constraints) -> RowWriterBuilder {
        let mut builder = GeneratedRowWriter::new_builder();
        for (name, value_set) in constraints.summary() {
            let value_set = value_set.clone();
            let projector: Arc<dyn ConstraintProjector> =
                Arc::new(move |value: &crate::data::Value| value_set.contains_value(value));
            builder.constraints.insert(name.clone(), projector);
        }
        builder
    }

    pub fn new_builder_with_evaluator(
        constraint_evaluator: Arc<ConstraintEvaluator>,
    ) -> RowWriterBuilder {
        let mut builder = GeneratedRowWriter::new_builder();
        builder.constraint_evaluator = Some(constraint_evaluator);
        builder
    }

    pub fn write_row(
        &mut self,
        block: &mut Block,
        row_num: usize,
        context: &dyn Any,
    ) -> Result<bool> {
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

    fn constraint_for(&self, field_name: &str) -> Option<Arc<dyn ConstraintProjector>> {
        if let Some(c) = self.constraints.get(field_name) {
            return Some(c.clone());
        }
        self.constraint_evaluator
            .as_ref()
            .and_then(|e| e.make_constraint_projector(field_name))
    }

    fn make_field_writer(&self, vector: &FieldVector) -> Result<Box<dyn FieldWriter>> {
        let field_name = vector.name().to_string();
        let field_type = vector.minor_type();
        let extractor = self.extractors.get(&field_name);
        let constraint = self.constraint_for(&field_name);
        if let Some(factory) = self.field_writer_factories.get(&field_name) {
            return factory.create(vector, extractor, constraint);
        }
        let extractor = extractor.ok_or_else(|| {
            FederationError::invalid_input(format!("Missing extractor for field[{field_name}]"))
        })?;
        let mismatch = || {
            FederationError::ClassCast(format!(
                "{} cannot be used for field {} of type {}",
                extractor.type_name(),
                field_name,
                field_type
            ))
        };
        Ok(match (field_type, extractor) {
            (MinorType::INT, Extractor::Int(e)) => {
                Box::new(IntFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::BIGINT, Extractor::BigInt(e)) => {
                Box::new(BigIntFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::DATEMILLI, Extractor::DateMilli(e)) => {
                Box::new(DateMilliFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::DATEDAY, Extractor::DateDay(e)) => {
                Box::new(DateDayFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::TINYINT, Extractor::TinyInt(e)) => {
                Box::new(TinyIntFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::SMALLINT, Extractor::SmallInt(e)) => {
                Box::new(SmallIntFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::FLOAT4, Extractor::Float4(e)) => {
                Box::new(Float4FieldWriter::new(e.clone(), constraint))
            }
            (MinorType::FLOAT8, Extractor::Float8(e)) => {
                Box::new(Float8FieldWriter::new(e.clone(), constraint))
            }
            (MinorType::DECIMAL, Extractor::Decimal(e)) => {
                Box::new(DecimalFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::BIT, Extractor::Bit(e)) => {
                Box::new(BitFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::VARCHAR, Extractor::VarChar(e)) => {
                Box::new(VarCharFieldWriter::new(e.clone(), constraint))
            }
            (MinorType::VARBINARY, Extractor::VarBinary(e)) => {
                Box::new(VarBinaryFieldWriter::new(e.clone(), constraint))
            }
            (
                MinorType::INT
                | MinorType::BIGINT
                | MinorType::DATEMILLI
                | MinorType::DATEDAY
                | MinorType::TINYINT
                | MinorType::SMALLINT
                | MinorType::FLOAT4
                | MinorType::FLOAT8
                | MinorType::DECIMAL
                | MinorType::BIT
                | MinorType::VARCHAR
                | MinorType::VARBINARY,
                _,
            ) => return Err(mismatch()),
            (other, _) => {
                return Err(FederationError::invalid_input(format!(
                    "{other} is not supported"
                )))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use arrow::datatypes::DataType;

    use super::*;
    use crate::data::block_allocator::{BlockAllocator, BlockAllocatorImpl};
    use crate::data::schema_builder::SchemaBuilder;
    use crate::data::value::Value;

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
            .with_extractor(
                "id",
                Extractor::int(|ctx: &dyn Any, dst: &mut crate::data::writers::holders::NullableIntHolder| {
                    let row = ctx.downcast_ref::<Row>().ok_or_else(|| FederationError::illegal_argument("bad context"))?;
                    dst.is_set = 1;
                    dst.value = row.id;
                    Ok(())
                }),
            )
            .with_extractor(
                "name",
                Extractor::var_char(|ctx: &dyn Any, dst: &mut crate::data::writers::holders::NullableVarCharHolder| {
                    let row = ctx.downcast_ref::<Row>().ok_or_else(|| FederationError::illegal_argument("bad context"))?;
                    match &row.name {
                        Some(n) => {
                            dst.is_set = 1;
                            dst.value = n.clone();
                        }
                        None => dst.is_set = 0,
                    }
                    Ok(())
                }),
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
        reader.set_position(0);
        assert_eq!(reader.read_text(), Some("a".to_string()));
        reader.set_position(1);
        assert!(!reader.is_set());
        let ids = block.get_field_reader("id").unwrap();
        ids.set_position(1);
        assert_eq!(ids.read_integer(), Some(2));
    }

    #[test]
    fn missing_extractor_is_an_error() {
        let schema = SchemaBuilder::new()
            .add_field_with_type("x", DataType::Int32)
            .build();
        let allocator = BlockAllocatorImpl::new();
        let mut block = allocator.create_block(Arc::new(schema));
        let mut writer = GeneratedRowWriter::new_builder().build();
        let err = writer.write_row(&mut block, 0, &()).unwrap_err();
        assert!(err.to_string().contains("Missing extractor for field[x]"));
    }
}
