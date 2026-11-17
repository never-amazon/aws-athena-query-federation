use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, BlockAllocator, Value};
use crate::exceptions::{FederationError, Result};

use super::marker::{objects_hash, type_hash, Marker};

#[derive(Debug, Clone)]
pub struct AllOrNoneValueSet {
    arrow_type: ArrowType,
    all: bool,
    null_allowed: bool,
}

impl AllOrNoneValueSet {
    pub fn new(arrow_type: ArrowType, all: bool, null_allowed: bool) -> AllOrNoneValueSet {
        AllOrNoneValueSet {
            arrow_type,
            all,
            null_allowed,
        }
    }

    pub fn all(arrow_type: &ArrowType) -> AllOrNoneValueSet {
        AllOrNoneValueSet::new(arrow_type.clone(), true, true)
    }

    pub fn none(arrow_type: &ArrowType) -> AllOrNoneValueSet {
        AllOrNoneValueSet::new(arrow_type.clone(), false, false)
    }

    pub fn only_null(arrow_type: &ArrowType) -> AllOrNoneValueSet {
        AllOrNoneValueSet::new(arrow_type.clone(), false, true)
    }

    pub fn not_null(arrow_type: &ArrowType) -> AllOrNoneValueSet {
        AllOrNoneValueSet::new(arrow_type.clone(), true, false)
    }

    pub fn is_null_allowed(&self) -> bool {
        self.null_allowed
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn is_none(&self) -> bool {
        !self.all && !self.null_allowed
    }

    pub fn is_all(&self) -> bool {
        self.all && self.null_allowed
    }

    pub fn get_all(&self) -> bool {
        self.all
    }

    pub fn is_single_value(&self) -> bool {
        false
    }

    pub fn get_single_value(&self) -> Result<Value> {
        Err(FederationError::not_supported("Operation not supported"))
    }

    pub fn contains_marker(&self, value: &Marker) -> Result<bool> {
        if value.is_null_value() {
            return Ok(self.null_allowed);
        }
        Ok(self.all)
    }

    pub fn contains_value(&self, value: &Value) -> Result<bool> {
        if value.is_null() {
            return Ok(self.null_allowed);
        }
        Ok(self.all)
    }

    pub fn intersect(&self, _allocator: &dyn BlockAllocator, other: &AllOrNoneValueSet) -> Result<AllOrNoneValueSet> {
        self.check_compatibility(other)?;
        Ok(AllOrNoneValueSet::new(
            self.arrow_type.clone(),
            self.all && other.all,
            self.null_allowed && other.null_allowed,
        ))
    }

    pub fn union(&self, _allocator: &dyn BlockAllocator, other: &AllOrNoneValueSet) -> Result<AllOrNoneValueSet> {
        self.check_compatibility(other)?;
        Ok(AllOrNoneValueSet::new(
            self.arrow_type.clone(),
            self.all || other.all,
            self.null_allowed || other.null_allowed,
        ))
    }

    pub fn union_all(&self, allocator: &dyn BlockAllocator, value_sets: &[AllOrNoneValueSet]) -> Result<AllOrNoneValueSet> {
        let mut current = self.clone();
        for v in value_sets {
            current = current.union(allocator, v)?;
        }
        Ok(current)
    }

    pub fn complement(&self, _allocator: &dyn BlockAllocator) -> Result<AllOrNoneValueSet> {
        Ok(AllOrNoneValueSet::new(self.arrow_type.clone(), !self.all, !self.null_allowed))
    }

    pub fn overlaps(&self, allocator: &dyn BlockAllocator, other: &AllOrNoneValueSet) -> Result<bool> {
        Ok(!self.intersect(allocator, other)?.is_none())
    }

    pub fn subtract(&self, allocator: &dyn BlockAllocator, other: &AllOrNoneValueSet) -> Result<AllOrNoneValueSet> {
        self.intersect(allocator, &other.complement(allocator)?)
    }

    pub fn contains(&self, allocator: &dyn BlockAllocator, other: &AllOrNoneValueSet) -> Result<bool> {
        Ok(self.union(allocator, other)? == *self)
    }

    fn check_compatibility(&self, other: &AllOrNoneValueSet) -> Result<()> {
        if self.arrow_type != other.arrow_type {
            return Err(FederationError::invalid_input(format!(
                "Mismatched types: {} vs {}",
                arrow_type_to_string(&self.arrow_type),
                arrow_type_to_string(&other.arrow_type)
            )));
        }
        Ok(())
    }

    pub fn hash_code(&self) -> i32 {
        objects_hash(&[type_hash(&self.arrow_type), if self.all { 1231 } else { 1237 }])
    }
}

impl PartialEq for AllOrNoneValueSet {
    fn eq(&self, other: &Self) -> bool {
        self.arrow_type == other.arrow_type && self.all == other.all
    }
}

impl fmt::Display for AllOrNoneValueSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{} nullAllowed:{}]", if self.all { "ALL" } else { "NONE" }, self.null_allowed)
    }
}
