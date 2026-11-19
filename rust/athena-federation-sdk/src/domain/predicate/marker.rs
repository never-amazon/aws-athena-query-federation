use crate::data::block_utils;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use arrow::datatypes::{Field, Schema};

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, ArrowTypeComparator, Block, BlockAllocator, Value};
use crate::exceptions::{FederationError, Result};

pub const DEFAULT_COLUMN: &str = "col1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Bound {
    Below,
    Exactly,
    Above,
}

impl Bound {
    pub fn name(&self) -> &'static str {
        match self {
            Bound::Below => "BELOW",
            Bound::Exactly => "EXACTLY",
            Bound::Above => "ABOVE",
        }
    }
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone)]
pub struct Marker {
    arrow_type: ArrowType,
    bound: Bound,
    null_value: bool,
    value: Value,
    allocator_id: Option<String>,
}

pub(crate) fn value_block_schema(arrow_type: &ArrowType) -> Arc<Schema> {
    Arc::new(Schema::new(vec![Field::new(
        DEFAULT_COLUMN,
        arrow_type.clone(),
        true,
    )]))
}

pub(crate) fn coerce_value(arrow_type: &ArrowType, value: &Value) -> Result<Value> {
    let mut block = Block::new("marker", value_block_schema(arrow_type));
    if let Some(vector) = block.get_field_vector_mut(DEFAULT_COLUMN) {
        block_utils::set_value(vector, 0, value)?;
    }
    block.set_row_count(1);
    Ok(block
        .get_field_vector(DEFAULT_COLUMN)
        .map_or(Value::Null, |v| v.get_object(0)))
}

impl Marker {
    pub fn from_value_block(value_block: &Block, bound: Bound, null_value: bool) -> Result<Marker> {
        Marker::from_block_position(value_block, 0, bound, null_value)
    }

    pub fn from_block_position(
        block: &Block,
        value_position: usize,
        bound: Bound,
        null_value: bool,
    ) -> Result<Marker> {
        let reader = block.get_field_reader(DEFAULT_COLUMN).ok_or_else(|| {
            FederationError::null_pointer(format!("block has no column {DEFAULT_COLUMN}"))
        })?;
        let arrow_type = reader.field().data_type().clone();
        let value = if null_value {
            Value::Null
        } else {
            reader.set_position(value_position);
            reader.read_object()
        };
        Ok(Marker {
            arrow_type,
            bound,
            null_value,
            value,
            allocator_id: Some(block.allocator_id().to_string()),
        })
    }

    pub(crate) fn from_parts(
        arrow_type: ArrowType,
        bound: Bound,
        null_value: bool,
        value: Value,
    ) -> Marker {
        Marker {
            arrow_type,
            bound,
            null_value,
            value,
            allocator_id: None,
        }
    }

    pub fn is_null_value(&self) -> bool {
        self.null_value
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn value(&self) -> Result<&Value> {
        if self.null_value {
            return Err(FederationError::invalid_input("No value to get"));
        }
        Ok(&self.value)
    }

    pub fn bound(&self) -> Bound {
        self.bound
    }

    pub fn schema(&self) -> Arc<Schema> {
        value_block_schema(&self.arrow_type)
    }

    pub fn allocator_id(&self) -> Option<&str> {
        self.allocator_id.as_deref()
    }

    pub fn get_value_block(&self) -> Result<Block> {
        let mut block = Block::new(
            self.allocator_id.as_deref().unwrap_or("marker"),
            value_block_schema(&self.arrow_type),
        );
        if !self.null_value {
            if let Some(vector) = block.get_field_vector_mut(DEFAULT_COLUMN) {
                vector.set(0, &self.value)?;
            }
            block.set_row_count(1);
        }
        Ok(block)
    }

    pub fn is_upper_unbounded(&self) -> bool {
        self.null_value && self.bound == Bound::Below
    }

    pub fn is_lower_unbounded(&self) -> bool {
        self.null_value && self.bound == Bound::Above
    }

    pub fn is_adjacent(&self, other: &Marker) -> Result<bool> {
        if self.is_upper_unbounded()
            || self.is_lower_unbounded()
            || other.is_upper_unbounded()
            || other.is_lower_unbounded()
        {
            return Ok(false);
        }
        if ArrowTypeComparator::compare(self.get_type(), self.value()?, other.value()?)? != 0 {
            return Ok(false);
        }
        Ok(
            (self.bound == Bound::Exactly && other.bound != Bound::Exactly)
                || (self.bound != Bound::Exactly && other.bound == Bound::Exactly),
        )
    }

    fn with_bound(&self, bound: Bound) -> Marker {
        Marker {
            arrow_type: self.arrow_type.clone(),
            bound,
            null_value: self.null_value,
            value: self.value.clone(),
            allocator_id: self.allocator_id.clone(),
        }
    }

    pub fn greater_adjacent(&self) -> Result<Marker> {
        if self.null_value {
            return Err(FederationError::invalid_input(
                "No marker adjacent to unbounded",
            ));
        }
        match self.bound {
            Bound::Below => Ok(self.with_bound(Bound::Exactly)),
            Bound::Exactly => Ok(self.with_bound(Bound::Above)),
            Bound::Above => Err(FederationError::invalid_input(
                "No greater marker adjacent to an ABOVE bound",
            )),
        }
    }

    pub fn lesser_adjacent(&self) -> Result<Marker> {
        if self.null_value {
            return Err(FederationError::invalid_input(
                "No marker adjacent to unbounded",
            ));
        }
        match self.bound {
            Bound::Below => Err(FederationError::invalid_input(
                "No lesser marker adjacent to a BELOW bound",
            )),
            Bound::Exactly => Ok(self.with_bound(Bound::Below)),
            Bound::Above => Ok(self.with_bound(Bound::Exactly)),
        }
    }

    pub fn min(marker1: &Marker, marker2: &Marker) -> Result<Marker> {
        Ok(if marker1.try_cmp(marker2)?.is_le() {
            marker1.clone()
        } else {
            marker2.clone()
        })
    }

    pub fn max(marker1: &Marker, marker2: &Marker) -> Result<Marker> {
        Ok(if marker1.try_cmp(marker2)?.is_ge() {
            marker1.clone()
        } else {
            marker2.clone()
        })
    }

    fn create_with_value(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        value: Value,
        bound: Bound,
    ) -> Result<Marker> {
        let block = block_utils::new_block(allocator, DEFAULT_COLUMN, arrow_type, vec![value])?;
        Marker::from_block_position(&block, 0, bound, false)
    }

    fn create_null(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, bound: Bound) -> Marker {
        let block = allocator.create_block(value_block_schema(arrow_type));
        Marker {
            arrow_type: arrow_type.clone(),
            bound,
            null_value: true,
            value: Value::Null,
            allocator_id: Some(block.allocator_id().to_string()),
        }
    }

    fn require_value(value: &Value) -> Result<()> {
        if value.is_null() {
            return Err(FederationError::null_pointer("value is null"));
        }
        Ok(())
    }

    pub fn upper_unbounded(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> Marker {
        Marker::create_null(allocator, arrow_type, Bound::Below)
    }

    pub fn lower_unbounded(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> Marker {
        Marker::create_null(allocator, arrow_type, Bound::Above)
    }

    pub fn above(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        value: impl Into<Value>,
    ) -> Result<Marker> {
        let value = value.into();
        Marker::require_value(&value)?;
        Marker::create_with_value(allocator, arrow_type, value, Bound::Above)
    }

    pub fn exactly(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        value: impl Into<Value>,
    ) -> Result<Marker> {
        let value = value.into();
        Marker::require_value(&value)?;
        Marker::create_with_value(allocator, arrow_type, value, Bound::Exactly)
    }

    pub fn null_marker(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> Marker {
        Marker::create_null(allocator, arrow_type, Bound::Exactly)
    }

    pub fn below(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        value: impl Into<Value>,
    ) -> Result<Marker> {
        let value = value.into();
        Marker::require_value(&value)?;
        Marker::create_with_value(allocator, arrow_type, value, Bound::Below)
    }

    pub fn literal(arrow_type: ArrowType, value: Value) -> Marker {
        Marker::from_parts(arrow_type, Bound::Exactly, value.is_null(), value)
    }

    pub fn nullable(arrow_type: &ArrowType, value: &Value, bound: Bound) -> Result<Marker> {
        if value.is_null() {
            return Ok(Marker::from_parts(
                arrow_type.clone(),
                bound,
                true,
                Value::Null,
            ));
        }
        let stored = coerce_value(arrow_type, value)?;
        Ok(Marker::from_parts(arrow_type.clone(), bound, false, stored))
    }

    pub fn try_cmp(&self, other: &Marker) -> Result<Ordering> {
        if self.is_upper_unbounded() {
            return Ok(if other.is_upper_unbounded() {
                Ordering::Equal
            } else {
                Ordering::Greater
            });
        }
        if self.is_lower_unbounded() {
            return Ok(if other.is_lower_unbounded() {
                Ordering::Equal
            } else {
                Ordering::Less
            });
        }
        if other.is_upper_unbounded() {
            return Ok(Ordering::Less);
        }
        if other.is_lower_unbounded() {
            return Ok(Ordering::Greater);
        }
        if self.null_value || other.null_value {
            return Ok(self.null_value.cmp(&other.null_value));
        }
        let compare = ArrowTypeComparator::compare(&self.arrow_type, &self.value, &other.value)?;
        if compare != 0 {
            return Ok(compare.cmp(&0));
        }
        Ok(match (self.bound, other.bound) {
            (Bound::Below, Bound::Below)
            | (Bound::Exactly, Bound::Exactly)
            | (Bound::Above, Bound::Above) => Ordering::Equal,
            (Bound::Below, _) | (Bound::Exactly, Bound::Above) => Ordering::Less,
            (Bound::Above, _) | (Bound::Exactly, Bound::Below) => Ordering::Greater,
        })
    }
}

impl PartialEq for Marker {
    fn eq(&self, other: &Self) -> bool {
        let result = self.null_value == other.null_value
            && self.arrow_type == other.arrow_type
            && self.bound == other.bound;
        if result && !self.null_value {
            return self.value == other.value;
        }
        result
    }
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = if self.null_value {
            "true".to_string()
        } else {
            self.value.java_to_string()
        };
        write!(
            f,
            "Marker{{valueBlock={}, nullValue={}, valueBlock={}, bound={}}}",
            arrow_type_to_string(&self.arrow_type),
            self.null_value,
            value,
            self.bound
        )
    }
}
