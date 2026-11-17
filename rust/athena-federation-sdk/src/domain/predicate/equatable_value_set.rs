use std::fmt;

use crate::data::types::{arrow_type_to_string, minor_type_of};
use crate::data::{ArrowType, ArrowTypeComparator, Block, BlockAllocator, BlockUtils, Value};
use crate::exceptions::{FederationError, Result};

use super::marker::{objects_hash, type_hash, Marker};

const DEFAULT_COLUMN: &str = "col1";

#[derive(Debug, Clone)]
pub struct EquatableValueSet {
    white_list: bool,
    value_block: Block,
    pub null_allowed: bool,
}

fn column_type(block: &Block) -> Result<ArrowType> {
    block
        .get_field_reader(DEFAULT_COLUMN)
        .map(|r| r.get_field().data_type().clone())
        .ok_or_else(|| {
            FederationError::NullPointer(format!("value block has no column {DEFAULT_COLUMN}"))
        })
}

fn block_values(block: &Block) -> Vec<Value> {
    match block.get_field_vector(DEFAULT_COLUMN) {
        Some(v) => (0..block.get_row_count())
            .map(|i| v.get_object(i))
            .collect(),
        None => Vec::new(),
    }
}

fn is_present(arrow_type: &ArrowType, lhs: &Value, right: &Block) -> Result<bool> {
    for rhs in block_values(right) {
        if ArrowTypeComparator::compare(arrow_type, lhs, &rhs)? == 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn build_block(
    allocator: &dyn BlockAllocator,
    arrow_type: &ArrowType,
    values: &[Value],
) -> Result<Block> {
    let mut block = BlockUtils::new_empty_block(allocator, DEFAULT_COLUMN, arrow_type);
    let count = values.len();
    if let Some(vector) = block.get_field_vector_mut(DEFAULT_COLUMN) {
        for (i, v) in values.iter().enumerate() {
            BlockUtils::set_value(vector, i, v)?;
        }
    }
    block.set_row_count(count);
    Ok(block)
}

impl EquatableValueSet {
    pub fn new(value_block: Block, white_list: bool, null_allowed: bool) -> EquatableValueSet {
        EquatableValueSet {
            white_list,
            value_block,
            null_allowed,
        }
    }

    pub fn new_builder<'a>(
        allocator: &'a dyn BlockAllocator,
        arrow_type: &ArrowType,
        is_white_list: bool,
        null_allowed: bool,
    ) -> EquatableValueSetBuilder<'a> {
        EquatableValueSetBuilder {
            allocator,
            arrow_type: arrow_type.clone(),
            is_white_list,
            null_allowed,
            values: Vec::new(),
        }
    }

    pub fn none(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> EquatableValueSet {
        EquatableValueSet::new(
            BlockUtils::new_empty_block(allocator, DEFAULT_COLUMN, arrow_type),
            true,
            false,
        )
    }

    pub fn all(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> EquatableValueSet {
        EquatableValueSet::new(
            BlockUtils::new_empty_block(allocator, DEFAULT_COLUMN, arrow_type),
            false,
            true,
        )
    }

    pub fn only_null(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> EquatableValueSet {
        EquatableValueSet::new(
            BlockUtils::new_empty_block(allocator, DEFAULT_COLUMN, arrow_type),
            false,
            true,
        )
    }

    pub fn not_null(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> EquatableValueSet {
        EquatableValueSet::new(
            BlockUtils::new_empty_block(allocator, DEFAULT_COLUMN, arrow_type),
            false,
            false,
        )
    }

    pub fn of<I, V>(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        values: I,
    ) -> Result<EquatableValueSet>
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        Ok(EquatableValueSet::new(
            BlockUtils::new_block(allocator, DEFAULT_COLUMN, arrow_type, values)?,
            true,
            false,
        ))
    }

    pub fn of_nullable<I, V>(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        null_allowed: bool,
        values: I,
    ) -> Result<EquatableValueSet>
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        Ok(EquatableValueSet::new(
            BlockUtils::new_block(allocator, DEFAULT_COLUMN, arrow_type, values)?,
            true,
            null_allowed,
        ))
    }

    pub fn is_null_allowed(&self) -> bool {
        self.null_allowed
    }

    pub fn get_schema(&self) -> &arrow::datatypes::SchemaRef {
        self.value_block.get_schema()
    }

    pub fn get_value_block(&self) -> &Block {
        &self.value_block
    }

    pub fn get_type(&self) -> ArrowType {
        column_type(&self.value_block).unwrap_or(ArrowType::Null)
    }

    pub fn is_white_list(&self) -> bool {
        self.white_list
    }

    pub fn get_values(&self) -> &Block {
        &self.value_block
    }

    pub fn get_value(&self, pos: usize) -> Value {
        self.value_block
            .get_field_vector(DEFAULT_COLUMN)
            .map_or(Value::Null, |v| v.get_object(pos))
    }

    pub fn is_none(&self) -> bool {
        self.white_list && self.value_block.get_row_count() == 0 && !self.null_allowed
    }

    pub fn is_all(&self) -> bool {
        !self.white_list && self.value_block.get_row_count() == 0 && self.null_allowed
    }

    pub fn is_single_value(&self) -> bool {
        let rows = self.value_block.get_row_count();
        self.white_list && ((rows == 1 && !self.null_allowed) || (rows == 0 && self.null_allowed))
    }

    pub fn get_single_value(&self) -> Result<Value> {
        if !self.is_single_value() {
            return Err(FederationError::invalid_input(
                "EquatableValueSet does not have just a single value",
            ));
        }
        if self.null_allowed && self.value_block.get_row_count() == 0 {
            return Ok(Value::Null);
        }
        Ok(self.get_value(0))
    }

    fn contains_raw(&self, value: &Value) -> Result<bool> {
        let arrow_type = self.get_type();
        let mut result = false;
        for rhs in block_values(&self.value_block) {
            if result {
                break;
            }
            result = ArrowTypeComparator::compare(&arrow_type, value, &rhs)? == 0;
        }
        Ok(self.white_list == result)
    }

    pub fn contains_marker(&self, marker: &Marker) -> Result<bool> {
        if marker.is_null_value() {
            return Ok(self.null_allowed);
        }
        self.contains_raw(marker.get_value()?)
    }

    pub fn contains_value(&self, value: &Value) -> Result<bool> {
        if value.is_null() && self.null_allowed {
            return Ok(true);
        }
        self.contains_raw(value)
    }

    fn intersect_blocks(
        allocator: &dyn BlockAllocator,
        left: &EquatableValueSet,
        right: &EquatableValueSet,
    ) -> Result<Block> {
        let arrow_type = left.get_type();
        let mut out = Vec::new();
        for v in block_values(&left.value_block) {
            if is_present(&column_type(&right.value_block)?, &v, &right.value_block)? {
                out.push(v);
            }
        }
        build_block(allocator, &arrow_type, &out)
    }

    fn union_blocks(
        allocator: &dyn BlockAllocator,
        left: &EquatableValueSet,
        right: &EquatableValueSet,
    ) -> Result<Block> {
        let arrow_type = left.get_type();
        let mut out = block_values(&left.value_block);
        for v in block_values(&right.value_block) {
            if !is_present(&column_type(&left.value_block)?, &v, &left.value_block)? {
                out.push(v);
            }
        }
        build_block(allocator, &arrow_type, &out)
    }

    fn subtract_blocks(
        allocator: &dyn BlockAllocator,
        left: &EquatableValueSet,
        right: &EquatableValueSet,
    ) -> Result<Block> {
        let arrow_type = left.get_type();
        let mut out = Vec::new();
        for v in block_values(&left.value_block) {
            if !is_present(&column_type(&right.value_block)?, &v, &right.value_block)? {
                out.push(v);
            }
        }
        build_block(allocator, &arrow_type, &out)
    }

    pub fn intersect(
        &self,
        allocator: &dyn BlockAllocator,
        other: &EquatableValueSet,
    ) -> Result<EquatableValueSet> {
        self.check_compatibility(other)?;
        let null_allowed = self.null_allowed && other.null_allowed;
        Ok(if self.white_list && other.white_list {
            EquatableValueSet::new(
                EquatableValueSet::intersect_blocks(allocator, self, other)?,
                true,
                null_allowed,
            )
        } else if self.white_list {
            EquatableValueSet::new(
                EquatableValueSet::subtract_blocks(allocator, self, other)?,
                true,
                null_allowed,
            )
        } else if other.white_list {
            EquatableValueSet::new(
                EquatableValueSet::subtract_blocks(allocator, other, self)?,
                true,
                null_allowed,
            )
        } else {
            EquatableValueSet::new(
                EquatableValueSet::union_blocks(allocator, other, self)?,
                false,
                null_allowed,
            )
        })
    }

    pub fn union(
        &self,
        allocator: &dyn BlockAllocator,
        other: &EquatableValueSet,
    ) -> Result<EquatableValueSet> {
        self.check_compatibility(other)?;
        let null_allowed = self.null_allowed || other.null_allowed;
        Ok(if self.white_list && other.white_list {
            EquatableValueSet::new(
                EquatableValueSet::union_blocks(allocator, other, self)?,
                true,
                null_allowed,
            )
        } else if self.white_list {
            EquatableValueSet::new(
                EquatableValueSet::subtract_blocks(allocator, other, self)?,
                false,
                null_allowed,
            )
        } else if other.white_list {
            EquatableValueSet::new(
                EquatableValueSet::subtract_blocks(allocator, self, other)?,
                false,
                null_allowed,
            )
        } else {
            EquatableValueSet::new(
                EquatableValueSet::intersect_blocks(allocator, other, self)?,
                false,
                null_allowed,
            )
        })
    }

    pub fn union_all(
        &self,
        allocator: &dyn BlockAllocator,
        value_sets: &[EquatableValueSet],
    ) -> Result<EquatableValueSet> {
        let mut current = self.clone();
        for v in value_sets {
            current = current.union(allocator, v)?;
        }
        Ok(current)
    }

    pub fn complement(&self, _allocator: &dyn BlockAllocator) -> Result<EquatableValueSet> {
        Ok(EquatableValueSet::new(
            self.value_block.clone(),
            !self.white_list,
            !self.null_allowed,
        ))
    }

    pub fn overlaps(
        &self,
        allocator: &dyn BlockAllocator,
        other: &EquatableValueSet,
    ) -> Result<bool> {
        Ok(!self.intersect(allocator, other)?.is_none())
    }

    pub fn subtract(
        &self,
        allocator: &dyn BlockAllocator,
        other: &EquatableValueSet,
    ) -> Result<EquatableValueSet> {
        self.intersect(allocator, &other.complement(allocator)?)
    }

    pub fn contains(
        &self,
        allocator: &dyn BlockAllocator,
        other: &EquatableValueSet,
    ) -> Result<bool> {
        Ok(self.union(allocator, other)? == *self)
    }

    fn check_compatibility(&self, other: &EquatableValueSet) -> Result<()> {
        if self.get_type() != other.get_type() {
            return Err(FederationError::invalid_input(format!(
                "Mismatched types: {} vs {}",
                arrow_type_to_string(&self.get_type()),
                arrow_type_to_string(&other.get_type())
            )));
        }
        Ok(())
    }

    pub fn hash_code(&self) -> i32 {
        objects_hash(&[
            type_hash(&self.get_type()),
            if self.white_list { 1231 } else { 1237 },
            self.value_block.hash_code(),
            if self.null_allowed { 1231 } else { 1237 },
        ])
    }
}

impl PartialEq for EquatableValueSet {
    fn eq(&self, other: &Self) -> bool {
        if minor_type_of(&self.get_type()) != minor_type_of(&other.get_type()) {
            return false;
        }
        if self.white_list != other.white_list {
            return false;
        }
        if self.null_allowed != other.null_allowed {
            return false;
        }
        self.value_block.equals_as_set(&other.value_block)
    }
}

impl fmt::Display for EquatableValueSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EquatableValueSet{{whiteList={}nullAllowed={}, valueBlock={}}}",
            self.white_list, self.null_allowed, self.value_block
        )
    }
}

pub struct EquatableValueSetBuilder<'a> {
    allocator: &'a dyn BlockAllocator,
    arrow_type: ArrowType,
    is_white_list: bool,
    null_allowed: bool,
    values: Vec<Value>,
}

impl EquatableValueSetBuilder<'_> {
    pub fn add(&mut self, value: impl Into<Value>) -> &mut Self {
        self.values.push(value.into());
        self
    }

    pub fn add_all<I, V>(&mut self, values: I) -> &mut Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.values.extend(values.into_iter().map(Into::into));
        self
    }

    pub fn build(&self) -> Result<EquatableValueSet> {
        Ok(EquatableValueSet::new(
            BlockUtils::new_block(
                self.allocator,
                DEFAULT_COLUMN,
                &self.arrow_type,
                self.values.clone(),
            )?,
            self.is_white_list,
            self.null_allowed,
        ))
    }
}
