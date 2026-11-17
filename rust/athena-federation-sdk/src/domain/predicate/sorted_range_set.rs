use std::cell::RefCell;
use std::fmt;

use arrow::datatypes::DataType;

use crate::data::types::arrow_type_to_string;
use crate::data::{ArrowType, BlockAllocator, Value};
use crate::exceptions::{FederationError, Result};

use super::marker::{objects_hash, Bound, Marker};
use super::range::Range;
use super::ranges::Ranges;
use super::value_marker::{LiteralValueMarker, ValueMarker};

#[derive(Debug, Clone)]
pub struct SortedRangeSet {
    null_allowed: bool,
    arrow_type: ArrowType,
    low_indexed_ranges: Vec<Range>,
}

impl SortedRangeSet {
    pub fn none(arrow_type: &ArrowType) -> SortedRangeSet {
        SortedRangeSet {
            null_allowed: false,
            arrow_type: arrow_type.clone(),
            low_indexed_ranges: Vec::new(),
        }
    }

    pub fn all(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> SortedRangeSet {
        SortedRangeSet {
            null_allowed: true,
            arrow_type: arrow_type.clone(),
            low_indexed_ranges: vec![Range::all(allocator, arrow_type)],
        }
    }

    pub fn only_null(arrow_type: &ArrowType) -> SortedRangeSet {
        SortedRangeSet {
            null_allowed: true,
            arrow_type: arrow_type.clone(),
            low_indexed_ranges: Vec::new(),
        }
    }

    pub fn not_null(allocator: &dyn BlockAllocator, arrow_type: &ArrowType) -> SortedRangeSet {
        SortedRangeSet {
            null_allowed: false,
            arrow_type: arrow_type.clone(),
            low_indexed_ranges: vec![Range::all(allocator, arrow_type)],
        }
    }

    pub fn of_values<I, V>(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        null_allowed: bool,
        first: impl Into<Value>,
        rest: I,
    ) -> Result<SortedRangeSet>
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        let mut ranges = vec![Range::equal(allocator, arrow_type, first)?];
        for value in rest {
            ranges.push(Range::equal(allocator, arrow_type, value)?);
        }
        SortedRangeSet::copy_of(arrow_type, ranges, null_allowed)
    }

    pub fn of(
        allocator: &dyn BlockAllocator,
        arrow_type: &ArrowType,
        first: impl Into<Value>,
    ) -> Result<SortedRangeSet> {
        SortedRangeSet::of_values(allocator, arrow_type, false, first, Vec::<Value>::new())
    }

    pub fn of_ranges(
        first: Range,
        rest: impl IntoIterator<Item = Range>,
    ) -> Result<SortedRangeSet> {
        SortedRangeSet::of_ranges_nullable(false, first, rest)
    }

    pub fn of_ranges_nullable(
        null_allowed: bool,
        first: Range,
        rest: impl IntoIterator<Item = Range>,
    ) -> Result<SortedRangeSet> {
        let arrow_type = first.get_type().clone();
        let mut ranges = vec![first];
        ranges.extend(rest);
        SortedRangeSet::copy_of(&arrow_type, ranges, null_allowed)
    }

    pub fn copy_of(
        arrow_type: &ArrowType,
        ranges: impl IntoIterator<Item = Range>,
        null_allowed: bool,
    ) -> Result<SortedRangeSet> {
        let mut builder = SortedRangeSetBuilder::new(arrow_type, null_allowed);
        builder.add_all(ranges)?;
        builder.build()
    }

    pub fn new_builder(arrow_type: &ArrowType, null_allowed: bool) -> SortedRangeSetBuilder {
        SortedRangeSetBuilder::new(arrow_type, null_allowed)
    }

    pub fn is_null_allowed(&self) -> bool {
        self.null_allowed
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn get_ordered_ranges(&self) -> Vec<Range> {
        self.low_indexed_ranges.clone()
    }

    pub fn ordered_ranges(&self) -> &[Range] {
        &self.low_indexed_ranges
    }

    pub fn range_count(&self) -> usize {
        self.low_indexed_ranges.len()
    }

    pub fn is_none(&self) -> bool {
        self.low_indexed_ranges.is_empty()
    }

    pub fn is_all(&self) -> bool {
        self.low_indexed_ranges.len() == 1 && self.low_indexed_ranges[0].is_all()
    }

    pub fn is_single_value(&self) -> bool {
        (self.low_indexed_ranges.len() == 1
            && self.low_indexed_ranges[0].is_single_value()
            && !self.null_allowed)
            || (self.low_indexed_ranges.is_empty() && self.null_allowed)
    }

    pub fn get_single_value(&self) -> Result<Value> {
        if !self.is_single_value() {
            return Err(FederationError::invalid_input(
                "SortedRangeSet does not have just a single value",
            ));
        }
        if self.null_allowed && self.low_indexed_ranges.is_empty() {
            return Ok(Value::Null);
        }
        self.low_indexed_ranges[0].get_single_value()
    }

    fn floor_entry(&self, marker: &dyn ValueMarker) -> Result<Option<&Range>> {
        let mut found = None;
        for range in &self.low_indexed_ranges {
            if range.low().compare_to(marker)? <= 0 {
                found = Some(range);
            } else {
                break;
            }
        }
        Ok(found)
    }

    pub fn contains_marker(&self, marker: &Marker) -> Result<bool> {
        self.check_type_compatibility(marker)?;
        if marker.is_null_value() {
            return Ok(self.null_allowed);
        }
        if marker.bound() != Bound::Exactly {
            return Err(FederationError::invalid_input(format!(
                "Expected Bound.EXACTLY but found {}",
                marker.bound()
            )));
        }
        match self.floor_entry(marker)? {
            Some(range) => range.includes(marker),
            None => Ok(false),
        }
    }

    pub fn contains_value(&self, value: &Value) -> Result<bool> {
        if value.is_null() {
            return Ok(self.null_allowed);
        }
        let marker = LiteralValueMarker::new(value.clone(), self.arrow_type.clone());
        match self.floor_entry(&marker)? {
            Some(range) => range.includes(&marker),
            None => Ok(false),
        }
    }

    pub fn includes_marker(&self, marker: &Marker) -> Result<bool> {
        self.check_type_compatibility(marker)?;
        if marker.is_null_value() && self.null_allowed {
            return Ok(true);
        }
        match self.floor_entry(marker)? {
            Some(range) => range.includes(marker),
            None => Ok(false),
        }
    }

    pub fn get_span(&self) -> Result<Range> {
        match (
            self.low_indexed_ranges.first(),
            self.low_indexed_ranges.last(),
        ) {
            (Some(first), Some(last)) => first.span(last),
            _ => Err(FederationError::invalid_input(
                "Can not get span if no ranges exist",
            )),
        }
    }

    pub fn intersect(
        &self,
        _allocator: &dyn BlockAllocator,
        other: &SortedRangeSet,
    ) -> Result<SortedRangeSet> {
        self.check_compatibility(other)?;
        let intersect_null_allowed = self.null_allowed && other.null_allowed;
        let mut builder = SortedRangeSetBuilder::new(&self.arrow_type, intersect_null_allowed);
        let mut it1 = self.low_indexed_ranges.iter();
        let mut it2 = other.low_indexed_ranges.iter();
        if let (Some(mut range1), Some(mut range2)) = (it1.next(), it2.next()) {
            loop {
                if range1.overlaps(range2)? {
                    builder.add(range1.intersect(range2)?)?;
                }
                if range1.high().compare_to(range2.high())? <= 0 {
                    match it1.next() {
                        Some(r) => range1 = r,
                        None => break,
                    }
                } else {
                    match it2.next() {
                        Some(r) => range2 = r,
                        None => break,
                    }
                }
            }
        }
        builder.build()
    }

    pub fn union(
        &self,
        _allocator: &dyn BlockAllocator,
        other: &SortedRangeSet,
    ) -> Result<SortedRangeSet> {
        let union_null_allowed = self.null_allowed || other.null_allowed;
        self.check_compatibility(other)?;
        let mut builder = SortedRangeSetBuilder::new(&self.arrow_type, union_null_allowed);
        builder.add_all(self.low_indexed_ranges.iter().cloned())?;
        builder.add_all(other.low_indexed_ranges.iter().cloned())?;
        builder.build()
    }

    pub fn union_all(
        &self,
        _allocator: &dyn BlockAllocator,
        value_sets: &[SortedRangeSet],
    ) -> Result<SortedRangeSet> {
        let mut union_null_allowed = self.null_allowed;
        for v in value_sets {
            union_null_allowed |= v.null_allowed;
        }
        let mut builder = SortedRangeSetBuilder::new(&self.arrow_type, union_null_allowed);
        builder.add_all(self.low_indexed_ranges.iter().cloned())?;
        for v in value_sets {
            self.check_compatibility(v)?;
            builder.add_all(v.low_indexed_ranges.iter().cloned())?;
        }
        builder.build()
    }

    pub fn complement(&self, allocator: &dyn BlockAllocator) -> Result<SortedRangeSet> {
        let mut builder = SortedRangeSetBuilder::new(&self.arrow_type, !self.null_allowed);
        let mut iter = self.low_indexed_ranges.iter();
        let Some(first) = iter.next() else {
            builder.add(Range::all(allocator, &self.arrow_type))?;
            return builder.build();
        };
        if !first.low().is_lower_unbounded() {
            builder.add(Range::new(
                Marker::lower_unbounded(allocator, &self.arrow_type),
                first.low().lesser_adjacent()?,
            )?)?;
        }
        let mut previous = first;
        for current in iter {
            let low = previous.high().greater_adjacent()?;
            let high = current.low().lesser_adjacent()?;
            builder.add(Range::new(low, high)?)?;
            previous = current;
        }
        if !previous.high().is_upper_unbounded() {
            builder.add(Range::new(
                previous.high().greater_adjacent()?,
                Marker::upper_unbounded(allocator, &self.arrow_type),
            )?)?;
        }
        builder.build()
    }

    pub fn overlaps(&self, allocator: &dyn BlockAllocator, other: &SortedRangeSet) -> Result<bool> {
        Ok(!self.intersect(allocator, other)?.is_none())
    }

    pub fn subtract(
        &self,
        allocator: &dyn BlockAllocator,
        other: &SortedRangeSet,
    ) -> Result<SortedRangeSet> {
        self.intersect(allocator, &other.complement(allocator)?)
    }

    pub fn contains(&self, allocator: &dyn BlockAllocator, other: &SortedRangeSet) -> Result<bool> {
        Ok(self.union(allocator, other)? == *self)
    }

    fn check_compatibility(&self, other: &SortedRangeSet) -> Result<()> {
        if self.arrow_type != other.arrow_type {
            return Err(FederationError::invalid_input(format!(
                "Mismatched types: {} vs {}",
                arrow_type_to_string(&self.arrow_type),
                arrow_type_to_string(&other.arrow_type)
            )));
        }
        Ok(())
    }

    fn check_type_compatibility(&self, marker: &Marker) -> Result<()> {
        let timestamp_ok = match (&self.arrow_type, marker.get_type()) {
            (DataType::Timestamp(a, _), DataType::Timestamp(b, _)) => a == b,
            _ => false,
        };
        if &self.arrow_type != marker.get_type() && !timestamp_ok {
            return Err(FederationError::invalid_input(format!(
                "Marker of {} does not match SortedRangeSet of {}",
                arrow_type_to_string(marker.get_type()),
                arrow_type_to_string(&self.arrow_type)
            )));
        }
        Ok(())
    }

    pub fn hash_code(&self) -> i32 {
        let map_hash = self.low_indexed_ranges.iter().fold(0i32, |acc, r| {
            acc.wrapping_add(r.low().hash_code() ^ r.hash_code())
        });
        objects_hash(&[map_hash, if self.null_allowed { 1231 } else { 1237 }])
    }
}

impl Ranges for SortedRangeSet {
    fn range_count(&self) -> usize {
        SortedRangeSet::range_count(self)
    }

    fn get_ordered_ranges(&self) -> Vec<Range> {
        SortedRangeSet::get_ordered_ranges(self)
    }

    fn get_span(&self) -> Result<Range> {
        SortedRangeSet::get_span(self)
    }
}

impl PartialEq for SortedRangeSet {
    fn eq(&self, other: &Self) -> bool {
        self.null_allowed == other.null_allowed
            && self.low_indexed_ranges == other.low_indexed_ranges
    }
}

impl fmt::Display for SortedRangeSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let entries: Vec<String> = self
            .low_indexed_ranges
            .iter()
            .map(|r| format!("{}={}", r.low(), r))
            .collect();
        write!(
            f,
            "SortedRangeSet{{type={}, nullAllowed={}, lowIndexedRanges={{{}}}}}",
            arrow_type_to_string(&self.arrow_type),
            self.null_allowed,
            entries.join(", ")
        )
    }
}

#[derive(Debug, Clone)]
pub struct SortedRangeSetBuilder {
    arrow_type: ArrowType,
    null_allowed: bool,
    ranges: Vec<Range>,
}

impl SortedRangeSetBuilder {
    pub fn new(arrow_type: &ArrowType, null_allowed: bool) -> SortedRangeSetBuilder {
        SortedRangeSetBuilder {
            arrow_type: arrow_type.clone(),
            null_allowed,
            ranges: Vec::new(),
        }
    }

    pub fn add(&mut self, range: Range) -> Result<&mut Self> {
        if &self.arrow_type != range.get_type() {
            return Err(FederationError::invalid_input(format!(
                "Range type {} does not match builder type {}",
                arrow_type_to_string(range.get_type()),
                arrow_type_to_string(&self.arrow_type)
            )));
        }
        self.ranges.push(range);
        Ok(self)
    }

    pub fn add_all(&mut self, ranges: impl IntoIterator<Item = Range>) -> Result<&mut Self> {
        for range in ranges {
            self.add(range)?;
        }
        Ok(self)
    }

    pub fn build(&self) -> Result<SortedRangeSet> {
        let error: RefCell<Option<FederationError>> = RefCell::new(None);
        let mut ranges = self.ranges.clone();
        ranges.sort_by(|a, b| match a.low().compare_to(b.low()) {
            Ok(c) => c.cmp(&0),
            Err(e) => {
                error.borrow_mut().get_or_insert(e);
                std::cmp::Ordering::Equal
            }
        });
        if let Some(e) = error.into_inner() {
            return Err(e);
        }
        let mut result: Vec<Range> = Vec::new();
        let mut current: Option<Range> = None;
        for next in ranges {
            current = match current {
                None => Some(next),
                Some(cur) => {
                    if cur.overlaps(&next)? || cur.high().is_adjacent(next.low())? {
                        Some(cur.span(&next)?)
                    } else {
                        result.push(cur);
                        Some(next)
                    }
                }
            };
        }
        if let Some(cur) = current {
            result.push(cur);
        }
        Ok(SortedRangeSet {
            null_allowed: self.null_allowed,
            arrow_type: self.arrow_type.clone(),
            low_indexed_ranges: result,
        })
    }
}
