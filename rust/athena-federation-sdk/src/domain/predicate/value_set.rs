use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, BlockAllocator, Value};
use crate::exceptions::{FederationError, Result};

use super::all_or_none_value_set::AllOrNoneValueSet;
use super::equatable_value_set::EquatableValueSet;
use super::marker::Marker;
use super::ranges::Ranges;
use super::sorted_range_set::SortedRangeSet;

#[derive(Debug, Clone, PartialEq)]
pub enum ValueSet {
    SortedRangeSet(SortedRangeSet),
    EquatableValueSet(EquatableValueSet),
    AllOrNoneValueSet(AllOrNoneValueSet),
}

impl From<SortedRangeSet> for ValueSet {
    fn from(v: SortedRangeSet) -> Self {
        ValueSet::SortedRangeSet(v)
    }
}

impl From<EquatableValueSet> for ValueSet {
    fn from(v: EquatableValueSet) -> Self {
        ValueSet::EquatableValueSet(v)
    }
}

impl From<AllOrNoneValueSet> for ValueSet {
    fn from(v: AllOrNoneValueSet) -> Self {
        ValueSet::AllOrNoneValueSet(v)
    }
}

impl ValueSet {
    pub fn type_name(&self) -> &'static str {
        match self {
            ValueSet::SortedRangeSet(_) => "sortable",
            ValueSet::EquatableValueSet(_) => "equatable",
            ValueSet::AllOrNoneValueSet(_) => "allOrNone",
        }
    }

    pub fn get_type(&self) -> ArrowType {
        match self {
            ValueSet::SortedRangeSet(v) => v.get_type().clone(),
            ValueSet::EquatableValueSet(v) => v.get_type(),
            ValueSet::AllOrNoneValueSet(v) => v.get_type().clone(),
        }
    }

    pub fn is_none(&self) -> bool {
        match self {
            ValueSet::SortedRangeSet(v) => v.is_none(),
            ValueSet::EquatableValueSet(v) => v.is_none(),
            ValueSet::AllOrNoneValueSet(v) => v.is_none(),
        }
    }

    pub fn is_all(&self) -> bool {
        match self {
            ValueSet::SortedRangeSet(v) => v.is_all(),
            ValueSet::EquatableValueSet(v) => v.is_all(),
            ValueSet::AllOrNoneValueSet(v) => v.is_all(),
        }
    }

    pub fn is_single_value(&self) -> bool {
        match self {
            ValueSet::SortedRangeSet(v) => v.is_single_value(),
            ValueSet::EquatableValueSet(v) => v.is_single_value(),
            ValueSet::AllOrNoneValueSet(v) => v.is_single_value(),
        }
    }

    pub fn get_single_value(&self) -> Result<Value> {
        match self {
            ValueSet::SortedRangeSet(v) => v.get_single_value(),
            ValueSet::EquatableValueSet(v) => v.get_single_value(),
            ValueSet::AllOrNoneValueSet(v) => v.get_single_value(),
        }
    }

    pub fn is_null_allowed(&self) -> bool {
        match self {
            ValueSet::SortedRangeSet(v) => v.is_null_allowed(),
            ValueSet::EquatableValueSet(v) => v.is_null_allowed(),
            ValueSet::AllOrNoneValueSet(v) => v.is_null_allowed(),
        }
    }

    pub fn contains_marker(&self, marker: &Marker) -> Result<bool> {
        match self {
            ValueSet::SortedRangeSet(v) => v.contains_marker(marker),
            ValueSet::EquatableValueSet(v) => v.contains_marker(marker),
            ValueSet::AllOrNoneValueSet(v) => v.contains_marker(marker),
        }
    }

    pub fn contains_value(&self, value: &Value) -> Result<bool> {
        match self {
            ValueSet::SortedRangeSet(v) => v.contains_value(value),
            ValueSet::EquatableValueSet(v) => v.contains_value(value),
            ValueSet::AllOrNoneValueSet(v) => v.contains_value(value),
        }
    }

    pub fn get_ranges(&self) -> Result<&dyn Ranges> {
        match self {
            ValueSet::SortedRangeSet(v) => Ok(v),
            _ => Err(FederationError::not_supported("Operation not supported")),
        }
    }

    pub fn as_sorted_range_set(&self) -> Option<&SortedRangeSet> {
        match self {
            ValueSet::SortedRangeSet(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_equatable_value_set(&self) -> Option<&EquatableValueSet> {
        match self {
            ValueSet::EquatableValueSet(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_all_or_none_value_set(&self) -> Option<&AllOrNoneValueSet> {
        match self {
            ValueSet::AllOrNoneValueSet(v) => Some(v),
            _ => None,
        }
    }

    fn mismatch(&self, other: &ValueSet) -> FederationError {
        if self.get_type() != other.get_type() {
            return FederationError::invalid_input(format!(
                "Mismatched types: {} vs {}",
                arrow_type_to_string(&self.get_type()),
                arrow_type_to_string(&other.get_type())
            ));
        }
        let name = match self {
            ValueSet::SortedRangeSet(_) => "SortedRangeSet",
            ValueSet::EquatableValueSet(_) => "EquatableValueSet",
            ValueSet::AllOrNoneValueSet(_) => "AllOrNoneValueSet",
        };
        FederationError::invalid_input(format!("ValueSet is not a {}: {}", name, other.type_name()))
    }

    pub fn intersect(&self, allocator: &dyn BlockAllocator, other: &ValueSet) -> Result<ValueSet> {
        match (self, other) {
            (ValueSet::SortedRangeSet(a), ValueSet::SortedRangeSet(b)) => {
                Ok(a.intersect(allocator, b)?.into())
            }
            (ValueSet::EquatableValueSet(a), ValueSet::EquatableValueSet(b)) => {
                Ok(a.intersect(allocator, b)?.into())
            }
            (ValueSet::AllOrNoneValueSet(a), ValueSet::AllOrNoneValueSet(b)) => {
                Ok(a.intersect(allocator, b)?.into())
            }
            _ => Err(self.mismatch(other)),
        }
    }

    pub fn union(&self, allocator: &dyn BlockAllocator, other: &ValueSet) -> Result<ValueSet> {
        match (self, other) {
            (ValueSet::SortedRangeSet(a), ValueSet::SortedRangeSet(b)) => {
                Ok(a.union(allocator, b)?.into())
            }
            (ValueSet::EquatableValueSet(a), ValueSet::EquatableValueSet(b)) => {
                Ok(a.union(allocator, b)?.into())
            }
            (ValueSet::AllOrNoneValueSet(a), ValueSet::AllOrNoneValueSet(b)) => {
                Ok(a.union(allocator, b)?.into())
            }
            _ => Err(self.mismatch(other)),
        }
    }

    pub fn union_all(
        &self,
        allocator: &dyn BlockAllocator,
        value_sets: &[ValueSet],
    ) -> Result<ValueSet> {
        if let ValueSet::SortedRangeSet(a) = self {
            let mut others = Vec::with_capacity(value_sets.len());
            for v in value_sets {
                match v {
                    ValueSet::SortedRangeSet(s) => others.push(s.clone()),
                    other => return Err(self.mismatch(other)),
                }
            }
            return Ok(a.union_all(allocator, &others)?.into());
        }
        let mut current = self.clone();
        for v in value_sets {
            current = current.union(allocator, v)?;
        }
        Ok(current)
    }

    pub fn complement(&self, allocator: &dyn BlockAllocator) -> Result<ValueSet> {
        Ok(match self {
            ValueSet::SortedRangeSet(v) => v.complement(allocator)?.into(),
            ValueSet::EquatableValueSet(v) => v.complement(allocator)?.into(),
            ValueSet::AllOrNoneValueSet(v) => v.complement(allocator)?.into(),
        })
    }

    pub fn overlaps(&self, allocator: &dyn BlockAllocator, other: &ValueSet) -> Result<bool> {
        Ok(!self.intersect(allocator, other)?.is_none())
    }

    pub fn subtract(&self, allocator: &dyn BlockAllocator, other: &ValueSet) -> Result<ValueSet> {
        self.intersect(allocator, &other.complement(allocator)?)
    }

    pub fn contains(&self, allocator: &dyn BlockAllocator, other: &ValueSet) -> Result<bool> {
        Ok(self.union(allocator, other)? == *self)
    }

    pub fn hash_code(&self) -> i32 {
        match self {
            ValueSet::SortedRangeSet(v) => v.hash_code(),
            ValueSet::EquatableValueSet(v) => v.hash_code(),
            ValueSet::AllOrNoneValueSet(v) => v.hash_code(),
        }
    }
}

impl fmt::Display for ValueSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueSet::SortedRangeSet(v) => write!(f, "{v}"),
            ValueSet::EquatableValueSet(v) => write!(f, "{v}"),
            ValueSet::AllOrNoneValueSet(v) => write!(f, "{v}"),
        }
    }
}
