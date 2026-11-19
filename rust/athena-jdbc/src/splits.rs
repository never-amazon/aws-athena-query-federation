use std::fmt;

use athena_federation_sdk::Result;

use crate::sql::{JdbcType, Row};
use crate::validate::{is_true, not_blank};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegerSplitter {
    split_info: SplitInfo<i32>,
    current: i32,
    step: i32,
    remainder: i32,
    current_split: i32,
}

impl IntegerSplitter {
    pub fn new(split_info: SplitInfo<i32>) -> Result<IntegerSplitter> {
        let low = *split_info.split_range().low();
        let high = *split_info.split_range().high();
        is_true(high >= low, "high is lower than low")?;
        let (step, remainder) = if high == low {
            (0, 1)
        } else {
            let diff = high.wrapping_sub(low).wrapping_add(1);
            let num_splits = split_info.num_splits();
            (diff.wrapping_div(num_splits), diff.wrapping_rem(num_splits))
        };
        Ok(IntegerSplitter {
            split_info,
            current: low,
            step,
            remainder,
            current_split: 1,
        })
    }

    fn high(&self) -> i32 {
        *self.split_info.split_range().high()
    }

    pub fn next_split(&mut self) -> SplitRange<i32> {
        let mut high = self.current.wrapping_add(self.step).wrapping_sub(1);
        if self.remainder >= self.current_split {
            high = high.wrapping_add(1);
        }
        let low = self.current;
        if high > self.high() {
            high = self.high();
        }
        self.current = high.wrapping_add(1);
        self.current_split = self.current_split.wrapping_add(1);
        SplitRange::new(low, high)
    }
}

impl Iterator for IntegerSplitter {
    type Item = SplitRange<i32>;

    fn next(&mut self) -> Option<SplitRange<i32>> {
        if self.has_next() {
            Some(self.next_split())
        } else {
            None
        }
    }
}

impl Splitter<i32> for IntegerSplitter {
    fn has_next(&self) -> bool {
        if self.step <= 1 {
            return self.current <= self.high();
        }
        self.current < self.high()
    }

    fn next_range_clause(&mut self) -> String {
        let split_range = self.next_split();
        let column_name = self.split_info.column_name();
        format!(
            "({column_name} >= {} AND {column_name} <= {})",
            split_range.low(),
            split_range.high()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SplitInfo<T> {
    split_range: SplitRange<T>,
    column_name: String,
    column_type: JdbcType,
    num_splits: i32,
}

impl<T> SplitInfo<T> {
    pub fn new(
        split_range: SplitRange<T>,
        column_name: &str,
        column_type: JdbcType,
        num_splits: i32,
    ) -> Result<SplitInfo<T>> {
        let column_name = not_blank(Some(column_name), "columnName must not be blank")?;
        Ok(SplitInfo {
            split_range,
            column_name: column_name.to_string(),
            column_type,
            num_splits: num_splits.max(1),
        })
    }

    pub fn split_range(&self) -> &SplitRange<T> {
        &self.split_range
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    pub fn column_type(&self) -> JdbcType {
        self.column_type
    }

    pub fn num_splits(&self) -> i32 {
        self.num_splits
    }
}

impl<T: fmt::Display> fmt::Display for SplitInfo<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SplitInfo{{splitRange={}, columnName='{}', columnType={}, numSplits={}}}",
            self.split_range,
            self.column_name,
            self.column_type.code(),
            self.num_splits
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SplitRange<T> {
    low: T,
    high: T,
}

impl<T> SplitRange<T> {
    pub fn new(low: T, high: T) -> SplitRange<T> {
        SplitRange { low, high }
    }

    pub fn low(&self) -> &T {
        &self.low
    }

    pub fn high(&self) -> &T {
        &self.high
    }
}

impl<T: fmt::Display> fmt::Display for SplitRange<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SplitRange{{low={}, high={}}}", self.low, self.high)
    }
}

pub trait Splitter<T>: Iterator<Item = SplitRange<T>> + Send {
    fn has_next(&self) -> bool;

    fn next_range_clause(&mut self) -> String;
}

pub fn splitter(
    column_name: &str,
    row: &Row,
    max_splits: i32,
) -> Result<Option<Box<dyn Splitter<i32>>>> {
    let column_type = row.metadata().column(0)?.jdbc_type();
    match column_type {
        JdbcType::Integer => {
            let low = row.value(0)?.to_int()?.unwrap_or(0);
            let high = row.value(1)?.to_int()?.unwrap_or(0);
            let split_info = SplitInfo::new(
                SplitRange::new(low, high),
                column_name,
                column_type,
                max_splits,
            )?;
            Ok(Some(Box::new(IntegerSplitter::new(split_info)?)))
        }
        _ => Ok(None),
    }
}
