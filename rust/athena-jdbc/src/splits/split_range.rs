use std::fmt;

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
