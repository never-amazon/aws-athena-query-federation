use athena_federation_sdk::Result;

use super::integer_splitter::IntegerSplitter;
use super::split_info::SplitInfo;
use super::split_range::SplitRange;
use super::splitter::Splitter;
use crate::sql::{JdbcType, Row};

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
