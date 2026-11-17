use arrow::datatypes::DataType;
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{Marker, Range, SortedRangeSet};
use athena_federation_sdk::FederationError;

const BIGINT: DataType = DataType::Int64;

fn is_connector_err<T>(r: Result<T, FederationError>) -> bool {
    matches!(r, Err(FederationError::AthenaConnector(_)))
}

struct H {
    a: BlockAllocatorImpl,
}

impl H {
    fn new() -> H {
        H { a: BlockAllocatorImpl::new() }
    }
    fn all(&self) -> SortedRangeSet {
        SortedRangeSet::all(&self.a, &BIGINT)
    }
    fn none(&self) -> SortedRangeSet {
        SortedRangeSet::none(&BIGINT)
    }
    fn of(&self, v: i64) -> SortedRangeSet {
        SortedRangeSet::of(&self.a, &BIGINT, v).unwrap()
    }
    fn set(&self, ranges: Vec<Range>) -> SortedRangeSet {
        let mut it = ranges.into_iter();
        let first = it.next().unwrap();
        SortedRangeSet::of_ranges(first, it).unwrap()
    }
    fn set_nullable(&self, ranges: Vec<Range>) -> SortedRangeSet {
        let mut it = ranges.into_iter();
        let first = it.next().unwrap();
        SortedRangeSet::of_ranges_nullable(true, first, it).unwrap()
    }
    fn r_all(&self) -> Range {
        Range::all(&self.a, &BIGINT)
    }
    fn eq(&self, v: i64) -> Range {
        Range::equal(&self.a, &BIGINT, v).unwrap()
    }
    fn gt(&self, v: i64) -> Range {
        Range::greater_than(&self.a, &BIGINT, v).unwrap()
    }
    fn ge(&self, v: i64) -> Range {
        Range::greater_than_or_equal(&self.a, &BIGINT, v).unwrap()
    }
    fn lt(&self, v: i64) -> Range {
        Range::less_than(&self.a, &BIGINT, v).unwrap()
    }
    fn le(&self, v: i64) -> Range {
        Range::less_than_or_equal(&self.a, &BIGINT, v).unwrap()
    }
    fn r(&self, l: i64, li: bool, h: i64, hi: bool) -> Range {
        Range::range(&self.a, &BIGINT, l, li, h, hi).unwrap()
    }
    fn exactly(&self, v: i64) -> Marker {
        Marker::exactly(&self.a, &BIGINT, v).unwrap()
    }
    fn lower(&self) -> Marker {
        Marker::lower_unbounded(&self.a, &BIGINT)
    }
    fn upper(&self) -> Marker {
        Marker::upper_unbounded(&self.a, &BIGINT)
    }
}

#[test]
fn test_empty_set() {
    let h = H::new();
    let range_set = h.none();
    assert_eq!(range_set.get_type(), &BIGINT);
    assert!(range_set.is_none());
    assert!(!range_set.is_all());
    assert!(!range_set.is_single_value());
    assert!(range_set.get_ordered_ranges().is_empty());
    assert_eq!(range_set.get_range_count(), 0);
    assert_eq!(range_set.complement(&h.a).unwrap(), h.all());
    assert!(!range_set.includes_marker(&h.lower()).unwrap());
    assert!(!range_set.includes_marker(&h.exactly(0)).unwrap());
    assert!(!range_set.includes_marker(&h.upper()).unwrap());
}

#[test]
fn test_entire_set() {
    let h = H::new();
    let range_set = h.all();
    assert_eq!(range_set.get_type(), &BIGINT);
    assert!(!range_set.is_none());
    assert!(range_set.is_all());
    assert!(!range_set.is_single_value());
    assert_eq!(range_set.get_range_count(), 1);
    assert_eq!(range_set.complement(&h.a).unwrap(), h.none());
    assert!(range_set.includes_marker(&h.lower()).unwrap());
    assert!(range_set.includes_marker(&h.exactly(0)).unwrap());
    assert!(range_set.includes_marker(&h.upper()).unwrap());
}

#[test]
fn test_nullability() {
    let h = H::new();
    let range_set = SortedRangeSet::of_values(&h.a, &BIGINT, true, 10i64, vec![10i64]).unwrap();
    assert!(range_set.contains_marker(&Marker::null_marker(&h.a, &BIGINT)).unwrap());
    assert!(!range_set.contains_marker(&h.exactly(1)).unwrap());
    assert!(range_set.contains_marker(&h.exactly(10)).unwrap());
}

#[test]
fn test_single_value() {
    let h = H::new();
    let range_set = h.of(10);
    let complement = h.set_nullable(vec![h.gt(10), h.lt(10)]);
    assert_eq!(range_set.get_type(), &BIGINT);
    assert!(!range_set.is_none());
    assert!(!range_set.is_all());
    assert!(range_set.is_single_value());
    assert_eq!(range_set.get_ordered_ranges(), vec![h.eq(10)]);
    assert_eq!(range_set.get_range_count(), 1);
    assert_eq!(range_set.complement(&h.a).unwrap(), complement);
    assert!(!range_set.includes_marker(&h.lower()).unwrap());
    assert!(range_set.includes_marker(&h.exactly(10)).unwrap());
    assert!(!range_set.includes_marker(&h.exactly(9)).unwrap());
    assert!(!range_set.includes_marker(&h.upper()).unwrap());
}

#[test]
fn test_bounded_set() {
    let h = H::new();
    let range_set = h.set(vec![
        h.eq(10),
        h.eq(0),
        h.r(9, true, 11, false),
        h.eq(0),
        h.r(2, true, 4, true),
        h.r(4, false, 5, true),
    ]);
    let normalized_result = vec![h.eq(0), h.r(2, true, 5, true), h.r(9, true, 11, false)];
    let complement = h.set_nullable(vec![h.lt(0), h.r(0, false, 2, false), h.r(5, false, 9, false), h.ge(11)]);
    assert_eq!(range_set.get_type(), &BIGINT);
    assert!(!range_set.is_none());
    assert!(!range_set.is_all());
    assert!(!range_set.is_single_value());
    assert_eq!(range_set.get_ordered_ranges(), normalized_result);
    assert_eq!(range_set, SortedRangeSet::copy_of(&BIGINT, normalized_result, false).unwrap());
    assert_eq!(range_set.get_range_count(), 3);
    assert_eq!(range_set.complement(&h.a).unwrap(), complement);
    assert!(!range_set.includes_marker(&h.lower()).unwrap());
    assert!(range_set.includes_marker(&h.exactly(0)).unwrap());
    assert!(!range_set.includes_marker(&h.exactly(1)).unwrap());
    assert!(!range_set.includes_marker(&h.exactly(7)).unwrap());
    assert!(range_set.includes_marker(&h.exactly(9)).unwrap());
    assert!(!range_set.includes_marker(&h.upper()).unwrap());
}

#[test]
fn test_unbounded_set() {
    let h = H::new();
    let range_set = h.set(vec![
        h.gt(10),
        h.le(0),
        h.r(2, true, 4, false),
        h.r(4, true, 6, false),
        h.r(1, false, 2, false),
        h.r(9, false, 11, false),
    ]);
    let normalized_result = vec![h.le(0), h.r(1, false, 6, false), h.gt(9)];
    let complement = h.set_nullable(vec![h.r(0, false, 1, true), h.r(6, true, 9, true)]);
    assert_eq!(range_set.get_type(), &BIGINT);
    assert!(!range_set.is_none());
    assert!(!range_set.is_all());
    assert!(!range_set.is_single_value());
    assert_eq!(range_set.get_ordered_ranges(), normalized_result);
    assert_eq!(range_set, SortedRangeSet::copy_of(&BIGINT, normalized_result, false).unwrap());
    assert_eq!(range_set.get_range_count(), 3);
    assert_eq!(range_set.complement(&h.a).unwrap(), complement);
    assert!(range_set.includes_marker(&h.lower()).unwrap());
    assert!(range_set.includes_marker(&h.exactly(0)).unwrap());
    assert!(range_set.includes_marker(&h.exactly(4)).unwrap());
    assert!(!range_set.includes_marker(&h.exactly(7)).unwrap());
    assert!(range_set.includes_marker(&h.upper()).unwrap());
}

#[test]
fn test_get_single_value() {
    let h = H::new();
    assert_eq!(h.of(0).get_single_value().unwrap(), Value::Long(0));
    assert!(is_connector_err(h.all().get_single_value()));
}

#[test]
fn test_span() {
    let h = H::new();
    assert!(is_connector_err(h.none().get_span()));
    assert_eq!(h.all().get_span().unwrap(), h.r_all());
    assert_eq!(h.of(0).get_span().unwrap(), h.eq(0));
    assert_eq!(h.set(vec![h.eq(0), h.eq(1)]).get_span().unwrap(), h.r(0, true, 1, true));
    assert_eq!(h.set(vec![h.eq(0), h.gt(1)]).get_span().unwrap(), h.ge(0));
    assert_eq!(h.set(vec![h.lt(0), h.gt(1)]).get_span().unwrap(), h.r_all());
}

#[test]
fn test_overlaps() {
    let h = H::new();
    let a = &h.a;
    assert!(h.all().overlaps(a, &h.all()).unwrap());
    assert!(!h.all().overlaps(a, &h.none()).unwrap());
    assert!(h.all().overlaps(a, &h.of(0)).unwrap());
    assert!(h.all().overlaps(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(h.all().overlaps(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(h.all().overlaps(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(!h.none().overlaps(a, &h.all()).unwrap());
    assert!(!h.none().overlaps(a, &h.none()).unwrap());
    assert!(!h.none().overlaps(a, &h.of(0)).unwrap());
    assert!(!h.none().overlaps(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(!h.none().overlaps(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(!h.none().overlaps(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(h.of(0).overlaps(a, &h.all()).unwrap());
    assert!(!h.of(0).overlaps(a, &h.none()).unwrap());
    assert!(h.of(0).overlaps(a, &h.of(0)).unwrap());
    assert!(h.of(0).overlaps(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(!h.of(0).overlaps(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(!h.of(0).overlaps(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(h.set(vec![h.eq(0), h.eq(1)]).overlaps(a, &h.set(vec![h.eq(1)])).unwrap());
    assert!(!h.set(vec![h.eq(0), h.eq(1)]).overlaps(a, &h.set(vec![h.eq(2)])).unwrap());
    assert!(h.set(vec![h.ge(0)]).overlaps(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(h.set(vec![h.gt(0)]).overlaps(a, &h.set(vec![h.ge(0)])).unwrap());
    assert!(!h.set(vec![h.lt(0)]).overlaps(a, &h.set(vec![h.gt(0)])).unwrap());
}

#[test]
fn test_contains() {
    let h = H::new();
    let a = &h.a;
    assert!(h.all().contains(a, &h.all()).unwrap());
    assert!(h.all().contains(a, &h.none()).unwrap());
    assert!(h.all().contains(a, &h.of(0)).unwrap());
    assert!(h.all().contains(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(h.all().contains(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(h.all().contains(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(!h.none().contains(a, &h.all()).unwrap());
    assert!(h.none().contains(a, &h.none()).unwrap());
    assert!(!h.none().contains(a, &h.of(0)).unwrap());
    assert!(!h.none().contains(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(!h.none().contains(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(!h.none().contains(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(!h.of(0).contains(a, &h.all()).unwrap());
    assert!(h.of(0).contains(a, &h.none()).unwrap());
    assert!(h.of(0).contains(a, &h.of(0)).unwrap());
    assert!(!h.of(0).contains(a, &h.set(vec![h.eq(0), h.eq(1)])).unwrap());
    assert!(!h.of(0).contains(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(!h.of(0).contains(a, &h.set(vec![h.gt(0), h.lt(0)])).unwrap());

    assert!(h.set(vec![h.eq(0), h.eq(1)]).contains(a, &h.set(vec![h.eq(1)])).unwrap());
    assert!(!h.set(vec![h.eq(0), h.eq(1)]).contains(a, &h.set(vec![h.eq(1), h.eq(2)])).unwrap());
    assert!(h.set(vec![h.ge(0)]).contains(a, &h.set(vec![h.gt(0)])).unwrap());
    assert!(!h.set(vec![h.gt(0)]).contains(a, &h.set(vec![h.ge(0)])).unwrap());
    assert!(!h.set(vec![h.lt(0)]).contains(a, &h.set(vec![h.gt(0)])).unwrap());
}

#[test]
fn test_intersect() {
    let h = H::new();
    let a = &h.a;
    assert_eq!(h.none().intersect(a, &h.none()).unwrap(), h.none());
    assert_eq!(h.all().intersect(a, &h.all()).unwrap(), h.all());
    assert_eq!(h.none().intersect(a, &h.all()).unwrap(), h.none());
    assert_eq!(
        h.set(vec![h.eq(1), h.eq(2), h.eq(3)]).intersect(a, &h.set(vec![h.eq(2), h.eq(4)])).unwrap(),
        h.set(vec![h.eq(2)])
    );
    assert_eq!(
        h.all().intersect(a, &h.set(vec![h.eq(2), h.eq(4)])).unwrap(),
        h.set(vec![h.eq(2), h.eq(4)])
    );
    assert_eq!(
        h.set(vec![h.r(0, true, 4, false)]).intersect(a, &h.set(vec![h.eq(2), h.gt(3)])).unwrap(),
        h.set(vec![h.eq(2), h.r(3, false, 4, false)])
    );
    assert_eq!(h.set(vec![h.ge(0)]).intersect(a, &h.set(vec![h.le(0)])).unwrap(), h.set(vec![h.eq(0)]));
    assert_eq!(
        h.set(vec![h.ge(-1)]).intersect(a, &h.set(vec![h.le(1)])).unwrap(),
        h.set(vec![h.r(-1, true, 1, true)])
    );
}

fn assert_union(h: &H, first: SortedRangeSet, second: SortedRangeSet, expected: SortedRangeSet) {
    assert_eq!(first.union(&h.a, &second).unwrap(), expected);
    assert_eq!(first.union_all(&h.a, &[first.clone(), second]).unwrap(), expected);
}

#[test]
fn test_union() {
    let h = H::new();
    assert_union(&h, h.none(), h.none(), h.none());
    assert_union(&h, h.all(), h.all(), h.all());
    assert_union(&h, h.none(), h.all(), h.all());
    assert_union(
        &h,
        h.set(vec![h.eq(1), h.eq(2)]),
        h.set(vec![h.eq(2), h.eq(3)]),
        h.set(vec![h.eq(1), h.eq(2), h.eq(3)]),
    );
    assert_union(&h, h.all(), h.set(vec![h.eq(0)]), h.all());
    assert_union(&h, h.set(vec![h.r(0, true, 4, false)]), h.set(vec![h.gt(3)]), h.set(vec![h.ge(0)]));
    assert_union(&h, h.set(vec![h.ge(0)]), h.set(vec![h.le(0)]), h.set(vec![h.r_all()]));
    assert_union(
        &h,
        h.set(vec![h.gt(0)]),
        h.set_nullable(vec![h.lt(0)]),
        h.of(0).complement(&h.a).unwrap(),
    );
}

#[test]
fn test_subtract() {
    let h = H::new();
    let a = &h.a;
    let s01 = || h.set(vec![h.eq(0), h.eq(1)]);
    let gt0 = || h.set(vec![h.gt(0)]);
    assert_eq!(h.all().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.all().subtract(a, &h.none()).unwrap(), h.all());
    assert_eq!(h.all().subtract(a, &h.of(0)).unwrap(), h.of(0).complement(a).unwrap());
    assert_eq!(h.all().subtract(a, &s01()).unwrap(), s01().complement(a).unwrap());
    assert_eq!(h.all().subtract(a, &gt0()).unwrap(), h.set_nullable(vec![h.le(0)]));

    assert_eq!(h.none().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.none()).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.of(0)).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &s01()).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &gt0()).unwrap(), h.none());

    assert_eq!(h.of(0).subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.of(0).subtract(a, &h.none()).unwrap(), h.of(0));
    assert_eq!(h.of(0).subtract(a, &h.of(0)).unwrap(), h.none());
    assert_eq!(h.of(0).subtract(a, &s01()).unwrap(), h.none());
    h.of(0).subtract(a, &gt0()).unwrap();
    assert_eq!(h.of(0).subtract(a, &gt0()).unwrap(), h.of(0));

    assert_eq!(s01().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(s01().subtract(a, &h.none()).unwrap(), s01());
    assert_eq!(s01().subtract(a, &h.of(0)).unwrap(), h.of(1));
    assert_eq!(s01().subtract(a, &s01()).unwrap(), h.none());
    assert_eq!(s01().subtract(a, &gt0()).unwrap(), h.set(vec![h.eq(0)]));

    assert_eq!(gt0().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(gt0().subtract(a, &h.none()).unwrap(), gt0());
    assert_eq!(gt0().subtract(a, &h.of(0)).unwrap(), gt0());
    assert_eq!(gt0().subtract(a, &s01()).unwrap(), h.set(vec![h.r(0, false, 1, false), h.gt(1)]));
    assert_eq!(gt0().subtract(a, &gt0()).unwrap(), h.none());
}
