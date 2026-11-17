use super::split_range::SplitRange;

pub trait Splitter<T>: Iterator<Item = SplitRange<T>> + Send {
    fn has_next(&self) -> bool;

    fn next_range_clause(&mut self) -> String;
}
