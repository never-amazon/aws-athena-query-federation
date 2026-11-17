use std::cmp::Ordering;

use bigdecimal::BigDecimal;

use super::values::{BitString, DateString, TimeString, TimestampString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RexUnknownAs {
    True,
    False,
    Unknown,
}

impl RexUnknownAs {
    pub fn negate(self) -> RexUnknownAs {
        match self {
            RexUnknownAs::True => RexUnknownAs::False,
            RexUnknownAs::False => RexUnknownAs::True,
            RexUnknownAs::Unknown => RexUnknownAs::Unknown,
        }
    }

    pub fn or(self, other: RexUnknownAs) -> RexUnknownAs {
        match self {
            RexUnknownAs::True => self,
            RexUnknownAs::Unknown => {
                if other == RexUnknownAs::True {
                    other
                } else {
                    self
                }
            }
            RexUnknownAs::False => other,
        }
    }

    pub fn to_boolean(self) -> Option<bool> {
        match self {
            RexUnknownAs::True => Some(true),
            RexUnknownAs::False => Some(false),
            RexUnknownAs::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SargValue {
    Boolean(bool),
    Exact(BigDecimal),
    Approx(f64),
    Char(String),
    Binary(BitString),
    Date(DateString),
    Time(TimeString),
    Timestamp(TimestampString),
}

impl SargValue {
    pub fn compare(&self, other: &SargValue) -> Ordering {
        match (self, other) {
            (SargValue::Boolean(a), SargValue::Boolean(b)) => a.cmp(b),
            (SargValue::Exact(a), SargValue::Exact(b)) => a.cmp(b),
            (SargValue::Approx(a), SargValue::Approx(b)) => {
                a.partial_cmp(b).unwrap_or(Ordering::Equal)
            }
            (SargValue::Exact(a), SargValue::Approx(b)) => {
                let fa: f64 = a.to_string().parse().unwrap_or(0.0);
                fa.partial_cmp(b).unwrap_or(Ordering::Equal)
            }
            (SargValue::Approx(a), SargValue::Exact(b)) => {
                let fb: f64 = b.to_string().parse().unwrap_or(0.0);
                a.partial_cmp(&fb).unwrap_or(Ordering::Equal)
            }
            (SargValue::Char(a), SargValue::Char(b)) => java_string_compare(a, b),
            (SargValue::Binary(a), SargValue::Binary(b)) => a.cmp(b),
            (SargValue::Date(a), SargValue::Date(b)) => a.cmp(b),
            (SargValue::Time(a), SargValue::Time(b)) => a.cmp(b),
            (SargValue::Timestamp(a), SargValue::Timestamp(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

fn java_string_compare(a: &str, b: &str) -> Ordering {
    let ua: Vec<u16> = a.encode_utf16().collect();
    let ub: Vec<u16> = b.encode_utf16().collect();
    ua.cmp(&ub)
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cut {
    BelowAll,
    Below(SargValue),
    Above(SargValue),
    AboveAll,
}

impl Cut {
    fn rank(&self) -> u8 {
        match self {
            Cut::BelowAll => 0,
            Cut::Below(_) | Cut::Above(_) => 1,
            Cut::AboveAll => 2,
        }
    }

    pub fn compare(&self, other: &Cut) -> Ordering {
        match self.rank().cmp(&other.rank()) {
            Ordering::Equal => {}
            o => return o,
        }
        match (self, other) {
            (Cut::Below(a), Cut::Below(b)) | (Cut::Above(a), Cut::Above(b)) => a.compare(b),
            (Cut::Below(a), Cut::Above(b)) => match a.compare(b) {
                Ordering::Equal => Ordering::Less,
                o => o,
            },
            (Cut::Above(a), Cut::Below(b)) => match a.compare(b) {
                Ordering::Equal => Ordering::Greater,
                o => o,
            },
            _ => Ordering::Equal,
        }
    }

    pub fn endpoint(&self) -> Option<&SargValue> {
        match self {
            Cut::Below(v) | Cut::Above(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Range {
    pub lower: Cut,
    pub upper: Cut,
}

impl Range {
    pub fn all() -> Range {
        Range {
            lower: Cut::BelowAll,
            upper: Cut::AboveAll,
        }
    }

    pub fn less_than(v: SargValue) -> Range {
        Range {
            lower: Cut::BelowAll,
            upper: Cut::Below(v),
        }
    }

    pub fn at_most(v: SargValue) -> Range {
        Range {
            lower: Cut::BelowAll,
            upper: Cut::Above(v),
        }
    }

    pub fn greater_than(v: SargValue) -> Range {
        Range {
            lower: Cut::Above(v),
            upper: Cut::AboveAll,
        }
    }

    pub fn at_least(v: SargValue) -> Range {
        Range {
            lower: Cut::Below(v),
            upper: Cut::AboveAll,
        }
    }

    pub fn singleton(v: SargValue) -> Range {
        Range {
            lower: Cut::Below(v.clone()),
            upper: Cut::Above(v),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lower.compare(&self.upper) != Ordering::Less
    }

    pub fn has_lower_bound(&self) -> bool {
        !matches!(self.lower, Cut::BelowAll)
    }

    pub fn has_upper_bound(&self) -> bool {
        !matches!(self.upper, Cut::AboveAll)
    }

    pub fn is_point(&self) -> bool {
        match (&self.lower, &self.upper) {
            (Cut::Below(a), Cut::Above(b)) => a.compare(b) == Ordering::Equal,
            _ => false,
        }
    }

    fn is_connected(&self, other: &Range) -> bool {
        self.lower.compare(&other.upper) != Ordering::Greater
            && other.lower.compare(&self.upper) != Ordering::Greater
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RangeSet {
    ranges: Vec<Range>,
}

impl RangeSet {
    pub fn empty() -> RangeSet {
        RangeSet { ranges: Vec::new() }
    }

    pub fn as_ranges(&self) -> &[Range] {
        &self.ranges
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn is_all(&self) -> bool {
        self.ranges.len() == 1 && self.ranges[0] == Range::all()
    }

    pub fn add(&mut self, range: Range) {
        if range.is_empty() {
            return;
        }
        let mut lower = range.lower;
        let mut upper = range.upper;
        let mut kept = Vec::with_capacity(self.ranges.len() + 1);
        let probe = Range {
            lower: lower.clone(),
            upper: upper.clone(),
        };
        for r in self.ranges.drain(..) {
            if r.is_connected(&probe) {
                if r.lower.compare(&lower) == Ordering::Less {
                    lower = r.lower.clone();
                }
                if r.upper.compare(&upper) == Ordering::Greater {
                    upper = r.upper.clone();
                }
            } else {
                kept.push(r);
            }
        }
        kept.push(Range { lower, upper });
        kept.sort_by(|a, b| a.lower.compare(&b.lower));
        let mut merged: Vec<Range> = Vec::with_capacity(kept.len());
        for r in kept {
            if let Some(last) = merged.last_mut() {
                if last.is_connected(&r) {
                    if r.upper.compare(&last.upper) == Ordering::Greater {
                        last.upper = r.upper;
                    }
                    continue;
                }
            }
            merged.push(r);
        }
        self.ranges = merged;
    }

    pub fn add_all(&mut self, other: &RangeSet) {
        for r in &other.ranges {
            self.add(r.clone());
        }
    }

    pub fn complement(&self) -> RangeSet {
        let mut out = Vec::new();
        let mut prev = Cut::BelowAll;
        for r in &self.ranges {
            let gap = Range {
                lower: prev.clone(),
                upper: r.lower.clone(),
            };
            if !gap.is_empty() {
                out.push(gap);
            }
            prev = r.upper.clone();
        }
        let tail = Range {
            lower: prev,
            upper: Cut::AboveAll,
        };
        if !tail.is_empty() {
            out.push(tail);
        }
        RangeSet { ranges: out }
    }

    pub fn span_encloses_all(&self) -> bool {
        match (self.ranges.first(), self.ranges.last()) {
            (Some(f), Some(l)) => {
                matches!(f.lower, Cut::BelowAll) && matches!(l.upper, Cut::AboveAll)
            }
            _ => false,
        }
    }

    pub fn count_points(&self) -> usize {
        self.ranges.iter().filter(|r| r.is_point()).count()
    }

    pub fn is_open_interval(&self) -> bool {
        if self.ranges.len() != 1 {
            return false;
        }
        let r = &self.ranges[0];
        !r.has_lower_bound() || !r.has_upper_bound()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sarg {
    pub range_set: RangeSet,
    pub null_as: RexUnknownAs,
}

impl Sarg {
    pub fn of(null_as: RexUnknownAs, range_set: RangeSet) -> Sarg {
        Sarg { range_set, null_as }
    }

    pub fn point_count(&self) -> usize {
        self.range_set.count_points()
    }

    pub fn is_all(&self) -> bool {
        self.range_set.is_all()
    }

    pub fn is_none(&self) -> bool {
        self.range_set.is_empty()
    }

    pub fn is_points(&self) -> bool {
        self.point_count() == self.range_set.as_ranges().len()
    }

    pub fn is_complemented_points(&self) -> bool {
        self.range_set.span_encloses_all()
            && !self.range_set.is_all()
            && self
                .range_set
                .complement()
                .as_ranges()
                .iter()
                .all(Range::is_point)
    }

    pub fn complexity(&self) -> usize {
        if self.is_all() {
            return if self.null_as == RexUnknownAs::True {
                2
            } else {
                1
            };
        }
        if self.is_none() {
            return usize::from(self.null_as != RexUnknownAs::False);
        }
        let complement = self.range_set.complement();
        let mut complexity = if self.range_set.as_ranges().len() == 2
            && complement.as_ranges().len() == 1
            && complement.as_ranges()[0].is_point()
        {
            1
        } else {
            self.range_set.as_ranges().len()
        };
        if self.null_as == RexUnknownAs::True {
            complexity += 1;
        }
        complexity
    }

    pub fn negate(&self) -> Sarg {
        Sarg {
            range_set: self.range_set.complement(),
            null_as: self.null_as.negate(),
        }
    }
}
