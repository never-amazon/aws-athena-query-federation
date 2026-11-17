use crate::exceptions::Result;

use super::range::Range;

pub trait Ranges {
    fn get_range_count(&self) -> usize;

    fn get_ordered_ranges(&self) -> Vec<Range>;

    fn get_span(&self) -> Result<Range>;
}
