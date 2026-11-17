use std::fmt;

use athena_federation_sdk::Result;

use super::split_range::SplitRange;
use crate::validate::not_blank;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SplitInfo<T> {
    split_range: SplitRange<T>,
    column_name: String,
    column_type: i32,
    num_splits: i32,
}

impl<T> SplitInfo<T> {
    pub fn new(
        split_range: SplitRange<T>,
        column_name: &str,
        column_type: i32,
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

    pub fn column_type(&self) -> i32 {
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
            self.split_range, self.column_name, self.column_type, self.num_splits
        )
    }
}
