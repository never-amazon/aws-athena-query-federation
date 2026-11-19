use athena_federation_sdk::Result;

use super::integer_splitter::IntegerSplitter;
use super::split_info::SplitInfo;
use super::split_range::SplitRange;
use super::splitter::Splitter;
use crate::sql::{JdbcType, ResultSet};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SplitterFactory;

impl SplitterFactory {
    pub fn new() -> SplitterFactory {
        SplitterFactory
    }

    pub fn get_splitter(
        &self,
        column_name: &str,
        result_set: &ResultSet,
        max_splits: i32,
    ) -> Result<Option<Box<dyn Splitter<i32>>>> {
        let column_type = result_set.metadata().column_type(1)?;
        match column_type {
            JdbcType::Integer => {
                let low = result_set.get_int(1usize)?.unwrap_or(0);
                let high = result_set.get_int(2usize)?.unwrap_or(0);
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
}
