use std::fmt;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, BlockAllocator, Value};
use crate::exceptions::{FederationError, Result};

use super::marker::{objects_hash, Bound, Marker};
use super::value_marker::ValueMarker;

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    low: Marker,
    high: Marker,
}

impl Range {
    pub fn new(low: Marker, high: Marker) -> Result<Range> {
        if low.get_type() != high.get_type() {
            return Err(FederationError::invalid_input(format!(
                "Marker types do not match: {} vs {}",
                arrow_type_to_string(low.get_type()),
                arrow_type_to_string(high.get_type())
            )));
        }
        if low.get_bound() == Bound::Below {
            return Err(FederationError::invalid_input("low bound must be EXACTLY or ABOVE"));
        }
        if high.get_bound() == Bound::Above {
            return Err(FederationError::invalid_input("high bound must be EXACTLY or BELOW"));
        }
        if low.compare_to(&high)? > 0 {
            return Err(FederationError::invalid_input("low must be less than or equal to high"));
        }
        Ok(Range { low, high })
    }

    pub fn all(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> Range {
        Range {
            low: Marker::lower_unbounded(allocator, arrow_type),
            high: Marker::upper_unbounded(allocator, arrow_type),
        }
    }

    pub fn greater_than(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, low: impl Into<Value>) -> Result<Range> {
        Range::new(Marker::above(allocator, arrow_type, low)?, Marker::upper_unbounded(allocator, arrow_type))
    }

    pub fn greater_than_or_equal(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, low: impl Into<Value>) -> Result<Range> {
        Range::new(Marker::exactly(allocator, arrow_type, low)?, Marker::upper_unbounded(allocator, arrow_type))
    }

    pub fn less_than(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, high: impl Into<Value>) -> Result<Range> {
        Range::new(Marker::lower_unbounded(allocator, arrow_type), Marker::below(allocator, arrow_type, high)?)
    }

    pub fn less_than_or_equal(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, high: impl Into<Value>) -> Result<Range> {
        Range::new(Marker::lower_unbounded(allocator, arrow_type), Marker::exactly(allocator, arrow_type, high)?)
    }

    pub fn equal(allocator: &dyn BlockAllocator, arrow_type: &ArrowType, value: impl Into<Value>) -> Result<Range> {
        let value = value.into();
        Range::new(
            Marker::exactly(allocator, arrow_type, value.clone())?,
            Marker::exactly(allocator, arrow_type, value)?,
        )
    }

    #[allow(clippy::self_named_constructors)]
    pub fn range(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        low: impl Into<Value>,
        low_inclusive: bool,
        high: impl Into<Value>,
        high_inclusive: bool,
    ) -> Result<Range> {
        let low_marker = if low_inclusive {
            Marker::exactly(allocator, arrow_type, low)?
        } else {
            Marker::above(allocator, arrow_type, low)?
        };
        let high_marker = if high_inclusive {
            Marker::exactly(allocator, arrow_type, high)?
        } else {
            Marker::below(allocator, arrow_type, high)?
        };
        Range::new(low_marker, high_marker)
    }

    pub fn get_type(&self) -> &ArrowType {
        self.low.get_type()
    }

    pub fn get_low(&self) -> &Marker {
        &self.low
    }

    pub fn get_high(&self) -> &Marker {
        &self.high
    }

    pub fn is_single_value(&self) -> bool {
        self.low.get_bound() == Bound::Exactly && self.low == self.high
    }

    pub fn get_single_value(&self) -> Result<Value> {
        if !self.is_single_value() {
            return Err(FederationError::invalid_input("Range does not have just a single value"));
        }
        self.low.get_value().cloned()
    }

    pub fn is_all(&self) -> bool {
        self.low.is_lower_unbounded() && self.high.is_upper_unbounded()
    }

    pub fn includes(&self, marker: &dyn ValueMarker) -> Result<bool> {
        Ok(self.low.compare_to(marker)? <= 0 && self.high.compare_to(marker)? >= 0)
    }

    pub fn contains(&self, other: &Range) -> Result<bool> {
        Ok(self.low.compare_to(other.get_low())? <= 0 && self.high.compare_to(other.get_high())? >= 0)
    }

    pub fn span(&self, other: &Range) -> Result<Range> {
        let low = Marker::min(&self.low, other.get_low())?;
        let high = Marker::max(&self.high, other.get_high())?;
        Range::new(low, high)
    }

    pub fn overlaps(&self, other: &Range) -> Result<bool> {
        Ok(self.low.compare_to(other.get_high())? <= 0 && other.get_low().compare_to(&self.high)? <= 0)
    }

    pub fn intersect(&self, other: &Range) -> Result<Range> {
        if !self.overlaps(other)? {
            return Err(FederationError::invalid_input("Cannot intersect non-overlapping ranges"));
        }
        let low = Marker::max(&self.low, other.get_low())?;
        let high = Marker::min(&self.high, other.get_high())?;
        Range::new(low, high)
    }

    pub fn hash_code(&self) -> i32 {
        objects_hash(&[self.low.hash_code(), self.high.hash_code()])
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Range{{low={}, high={}}}", self.low, self.high)
    }
}
