use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::Schema;

use super::constraint_projector::ConstraintProjector;
use super::constraints::Constraints;
use super::marker::Bound;
use super::marker_factory::MarkerFactory;
use crate::data::{ArrowType, BlockAllocator, Value};
use crate::exceptions::Result;

#[derive(Debug, Default)]
pub struct ConstraintEvaluator {
    constraints: Constraints,
    marker_factory: MarkerFactory,
    type_map: HashMap<String, ArrowType>,
}

impl ConstraintEvaluator {
    pub fn new(
        _allocator: Option<&dyn BlockAllocator>,
        schema: &Schema,
        constraints: Constraints,
    ) -> ConstraintEvaluator {
        let type_map = schema
            .fields()
            .iter()
            .map(|f| (f.name().clone(), f.data_type().clone()))
            .collect();
        ConstraintEvaluator {
            constraints,
            marker_factory: MarkerFactory::new(),
            type_map,
        }
    }

    pub fn empty_evaluator() -> ConstraintEvaluator {
        ConstraintEvaluator::new(None, &Schema::empty(), Constraints::empty())
    }

    pub fn get_constraints(&self) -> &Constraints {
        &self.constraints
    }

    pub fn apply(&self, field_name: &str, value: &Value) -> Result<bool> {
        let constraint = self.constraints.get_summary().get(field_name);
        if let (Some(constraint), Some(arrow_type)) = (constraint, self.type_map.get(field_name)) {
            let marker = self
                .marker_factory
                .create_nullable(arrow_type, value, Bound::Exactly)?;
            return constraint.contains_marker(&marker);
        }
        Ok(true)
    }

    pub fn make_constraint_projector(
        &self,
        field_name: &str,
    ) -> Option<Arc<dyn ConstraintProjector>> {
        let constraint = self.constraints.get_summary().get(field_name)?;
        self.type_map.get(field_name)?;
        let constraint = constraint.clone();
        Some(Arc::new(move |value: &Value| {
            constraint.contains_value(value)
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::DataType;
    use indexmap::IndexMap;

    use super::*;
    use crate::data::{BlockAllocatorImpl, SchemaBuilder};
    use crate::domain::predicate::{EquatableValueSet, ValueSet};

    #[test]
    fn evaluator_filters_block_values() {
        let allocator = BlockAllocatorImpl::new();
        let schema = SchemaBuilder::new_builder()
            .add_int_field("col1")
            .add_int_field("col2")
            .build();
        let mut block = allocator.create_block_from_schema(&schema);
        let mut builder = EquatableValueSet::new_builder(&allocator, &DataType::Int32, true, false);
        builder.add(10);
        let set: ValueSet = builder.build().unwrap().into();
        let mut summary = IndexMap::new();
        summary.insert("col1".to_string(), set);
        let constraints = Constraints::new(summary, vec![], vec![], -1, HashMap::new(), None);
        let evaluator = Arc::new(ConstraintEvaluator::new(
            Some(&allocator),
            &schema,
            constraints,
        ));
        block.constrain(Some(evaluator.clone()));
        assert!(block.set_value("col1", 0, 10).unwrap());
        assert!(block.offer_value("col1", 0, 10).unwrap());
        assert!(!block.set_value("col1", 0, 11).unwrap());
        assert!(!block.offer_value("col1", 0, 11).unwrap());
        assert!(block.offer_value("unkown_col", 0, 10).unwrap());
        let projector = evaluator.make_constraint_projector("col1").unwrap();
        assert!(projector.apply(&Value::Int(10)).unwrap());
        assert!(!projector.apply(&Value::Int(11)).unwrap());
        assert!(evaluator.make_constraint_projector("col2").is_none());
    }
}
