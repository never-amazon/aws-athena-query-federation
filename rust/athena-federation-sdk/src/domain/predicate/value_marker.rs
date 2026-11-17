use crate::data::arrow_type_comparator::ArrowTypeComparator;
use crate::data::{ArrowType, Value};
use crate::exceptions::Result;

use super::marker::Bound;

pub trait ValueMarker {
    fn is_upper_unbounded(&self) -> bool;

    fn is_lower_unbounded(&self) -> bool;

    fn is_null_value(&self) -> bool;

    fn value_ref(&self) -> &Value;

    fn bound(&self) -> Bound;

    fn get_type(&self) -> &ArrowType;
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiteralValueMarker {
    value: Value,
    arrow_type: ArrowType,
}

impl LiteralValueMarker {
    pub fn new(value: impl Into<Value>, arrow_type: ArrowType) -> LiteralValueMarker {
        LiteralValueMarker {
            value: value.into(),
            arrow_type,
        }
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn compare_to(&self, other: &dyn ValueMarker) -> Result<i32> {
        ValueMarkerComparator::do_compare(self, other)
    }
}

impl ValueMarker for LiteralValueMarker {
    fn is_upper_unbounded(&self) -> bool {
        false
    }

    fn is_lower_unbounded(&self) -> bool {
        false
    }

    fn is_null_value(&self) -> bool {
        self.value.is_null()
    }

    fn value_ref(&self) -> &Value {
        &self.value
    }

    fn bound(&self) -> Bound {
        Bound::Exactly
    }

    fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }
}

pub struct ValueMarkerComparator;

impl ValueMarkerComparator {
    pub fn do_compare(o1: &dyn ValueMarker, o2: &dyn ValueMarker) -> Result<i32> {
        ValueMarkerComparator.compare(o1, o2)
    }

    pub fn compare(&self, o1: &dyn ValueMarker, o2: &dyn ValueMarker) -> Result<i32> {
        if o1.is_upper_unbounded() {
            return Ok(i32::from(!o2.is_upper_unbounded()));
        }
        if o1.is_lower_unbounded() {
            return Ok(if o2.is_lower_unbounded() { 0 } else { -1 });
        }
        if o2.is_upper_unbounded() {
            return Ok(-1);
        }
        if o2.is_lower_unbounded() {
            return Ok(1);
        }
        if o1.is_null_value() || o2.is_null_value() {
            if o1.is_null_value() == o2.is_null_value() {
                return Ok(0);
            }
            return Ok(if o1.is_null_value() && !o2.is_null_value() {
                1
            } else {
                -1
            });
        }
        let compare = ArrowTypeComparator::compare(o1.get_type(), o1.value_ref(), o2.value_ref())?;
        if compare == 0 {
            if o1.bound() == o2.bound() {
                return Ok(0);
            }
            if o1.bound() == Bound::Below {
                return Ok(-1);
            }
            if o1.bound() == Bound::Above {
                return Ok(1);
            }
            return Ok(if o2.bound() == Bound::Below { 1 } else { -1 });
        }
        Ok(compare)
    }
}
