use athena_federation_sdk::Result;

use super::split_info::SplitInfo;
use super::split_range::SplitRange;
use super::splitter::Splitter;
use crate::validate::is_true;

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
