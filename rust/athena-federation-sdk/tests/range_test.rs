use arrow::datatypes::DataType;
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{LiteralValueMarker, Marker, Range};
use athena_federation_sdk::FederationError;

const BIGINT: DataType = DataType::Int64;

fn is_connector_err<T>(r: &Result<T, FederationError>) -> bool {
    matches!(r, Err(FederationError::AthenaConnector(_)))
}

fn lit(v: i64) -> LiteralValueMarker {
    LiteralValueMarker::new(v, BIGINT)
}

#[test]
fn test_mismatched_types() {
    let a = BlockAllocatorImpl::new();
    assert!(is_connector_err(&Range::new(
        Marker::exactly(&a, &BIGINT, 1i64).unwrap(),
        Marker::exactly(&a, &DataType::Utf8, "a").unwrap()
    )));
}

#[test]
fn test_inverted_bounds() {
    let a = BlockAllocatorImpl::new();
    assert!(is_connector_err(&Range::new(
        Marker::exactly(&a, &BIGINT, 1i64).unwrap(),
        Marker::exactly(&a, &BIGINT, 0i64).unwrap()
    )));
}

#[test]
fn test_lower_unbounded_only() {
    let a = BlockAllocatorImpl::new();
    assert!(is_connector_err(&Range::new(
        Marker::lower_unbounded(&a, &BIGINT),
        Marker::lower_unbounded(&a, &BIGINT)
    )));
}

#[test]
fn test_upper_unbounded_only() {
    let a = BlockAllocatorImpl::new();
    assert!(is_connector_err(&Range::new(
        Marker::upper_unbounded(&a, &BIGINT),
        Marker::upper_unbounded(&a, &BIGINT)
    )));
}

#[test]
fn test_single_value() {
    let a = BlockAllocatorImpl::new();
    assert!(Range::range(&a, &BIGINT, 1i64, true, 1i64, true)
        .unwrap()
        .is_single_value());
    assert!(!Range::range(&a, &BIGINT, 1i64, true, 2i64, true)
        .unwrap()
        .is_single_value());
    assert!(Range::range(&a, &DataType::Float64, 1.1, true, 1.1, true)
        .unwrap()
        .is_single_value());
    assert!(Range::range(&a, &DataType::Utf8, "a", true, "a", true)
        .unwrap()
        .is_single_value());
    assert!(Range::range(&a, &DataType::Boolean, true, true, true, true)
        .unwrap()
        .is_single_value());
    assert!(
        !Range::range(&a, &DataType::Boolean, false, true, true, true)
            .unwrap()
            .is_single_value()
    );
}

#[test]
fn test_all_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::all(&a, &BIGINT);
    assert_eq!(range.get_low(), &Marker::lower_unbounded(&a, &BIGINT));
    assert_eq!(range.get_high(), &Marker::upper_unbounded(&a, &BIGINT));
    assert!(!range.is_single_value());
    assert!(range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(range
        .includes(&Marker::below(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::above(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_greater_than_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::greater_than(&a, &BIGINT, 1i64).unwrap();
    assert_eq!(range.get_low(), &Marker::above(&a, &BIGINT, 1i64).unwrap());
    assert_eq!(range.get_high(), &Marker::upper_unbounded(&a, &BIGINT));
    assert!(!range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(!range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 2i64).unwrap())
        .unwrap());
    assert!(range.includes(&lit(2)).unwrap());
    assert!(range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_greater_than_or_equal_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::greater_than_or_equal(&a, &BIGINT, 1i64).unwrap();
    assert_eq!(
        range.get_low(),
        &Marker::exactly(&a, &BIGINT, 1i64).unwrap()
    );
    assert_eq!(range.get_high(), &Marker::upper_unbounded(&a, &BIGINT));
    assert!(!range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(!range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 0i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 2i64).unwrap())
        .unwrap());
    assert!(range.includes(&lit(2)).unwrap());
    assert!(range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_less_than_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::less_than(&a, &BIGINT, 1i64).unwrap();
    assert_eq!(range.get_low(), &Marker::lower_unbounded(&a, &BIGINT));
    assert_eq!(range.get_high(), &Marker::below(&a, &BIGINT, 1i64).unwrap());
    assert!(!range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 0i64).unwrap())
        .unwrap());
    assert!(range.includes(&lit(0)).unwrap());
    assert!(!range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_less_than_or_equal_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::less_than_or_equal(&a, &BIGINT, 1i64).unwrap();
    assert_eq!(range.get_low(), &Marker::lower_unbounded(&a, &BIGINT));
    assert_eq!(
        range.get_high(),
        &Marker::exactly(&a, &BIGINT, 1i64).unwrap()
    );
    assert!(!range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 2i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 0i64).unwrap())
        .unwrap());
    assert!(!range.includes(&lit(2)).unwrap());
    assert!(!range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_equal_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::equal(&a, &BIGINT, 1i64).unwrap();
    assert_eq!(
        range.get_low(),
        &Marker::exactly(&a, &BIGINT, 1i64).unwrap()
    );
    assert_eq!(
        range.get_high(),
        &Marker::exactly(&a, &BIGINT, 1i64).unwrap()
    );
    assert!(range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(!range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 0i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 2i64).unwrap())
        .unwrap());
    assert!(!range.includes(&lit(2)).unwrap());
    assert!(!range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_range() {
    let a = BlockAllocatorImpl::new();
    let range = Range::range(&a, &BIGINT, 0i64, false, 2i64, true).unwrap();
    assert_eq!(range.get_low(), &Marker::above(&a, &BIGINT, 0i64).unwrap());
    assert_eq!(
        range.get_high(),
        &Marker::exactly(&a, &BIGINT, 2i64).unwrap()
    );
    assert!(!range.is_single_value());
    assert!(!range.is_all());
    assert_eq!(range.get_type(), &BIGINT);
    assert!(!range
        .includes(&Marker::lower_unbounded(&a, &BIGINT))
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 0i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 1i64).unwrap())
        .unwrap());
    assert!(range
        .includes(&Marker::exactly(&a, &BIGINT, 2i64).unwrap())
        .unwrap());
    assert!(!range
        .includes(&Marker::exactly(&a, &BIGINT, 3i64).unwrap())
        .unwrap());
    assert!(!range.includes(&lit(0)).unwrap());
    assert!(range.includes(&lit(1)).unwrap());
    assert!(range.includes(&lit(2)).unwrap());
    assert!(!range.includes(&lit(3)).unwrap());
    assert!(!range
        .includes(&Marker::upper_unbounded(&a, &BIGINT))
        .unwrap());
}

#[test]
fn test_get_single_value() {
    let a = BlockAllocatorImpl::new();
    assert_eq!(
        Range::equal(&a, &BIGINT, 0i64)
            .unwrap()
            .get_single_value()
            .unwrap(),
        Value::Long(0)
    );
    assert!(is_connector_err(
        &Range::less_than(&a, &BIGINT, 0i64)
            .unwrap()
            .get_single_value()
    ));
}

#[test]
fn test_contains() {
    let a = BlockAllocatorImpl::new();
    let all = || Range::all(&a, &BIGINT);
    let eq = |v: i64| Range::equal(&a, &BIGINT, v).unwrap();
    let gt = |v: i64| Range::greater_than(&a, &BIGINT, v).unwrap();
    let ge = |v: i64| Range::greater_than_or_equal(&a, &BIGINT, v).unwrap();
    let lt = |v: i64| Range::less_than(&a, &BIGINT, v).unwrap();
    let r = |l: i64, li: bool, h: i64, hi: bool| Range::range(&a, &BIGINT, l, li, h, hi).unwrap();
    assert!(all().contains(&all()).unwrap());
    assert!(all().contains(&eq(0)).unwrap());
    assert!(all().contains(&gt(0)).unwrap());
    assert!(eq(0).contains(&eq(0)).unwrap());
    assert!(!eq(0).contains(&gt(0)).unwrap());
    assert!(!eq(0).contains(&ge(0)).unwrap());
    assert!(!eq(0).contains(&all()).unwrap());
    assert!(ge(0).contains(&gt(0)).unwrap());
    assert!(gt(0).contains(&gt(1)).unwrap());
    assert!(!gt(0).contains(&lt(0)).unwrap());
    assert!(r(0, true, 2, true).contains(&r(1, true, 2, true)).unwrap());
    assert!(!r(0, true, 2, true).contains(&r(1, true, 3, false)).unwrap());
}

#[test]
fn test_span() {
    let a = BlockAllocatorImpl::new();
    let all = || Range::all(&a, &BIGINT);
    let eq = |v: i64| Range::equal(&a, &BIGINT, v).unwrap();
    let gt = |v: i64| Range::greater_than(&a, &BIGINT, v).unwrap();
    let ge = |v: i64| Range::greater_than_or_equal(&a, &BIGINT, v).unwrap();
    let lt = |v: i64| Range::less_than(&a, &BIGINT, v).unwrap();
    let le = |v: i64| Range::less_than_or_equal(&a, &BIGINT, v).unwrap();
    let r = |l: i64, li: bool, h: i64, hi: bool| Range::range(&a, &BIGINT, l, li, h, hi).unwrap();
    assert_eq!(gt(1).span(&le(2)).unwrap(), all());
    assert_eq!(gt(2).span(&le(0)).unwrap(), all());
    assert_eq!(
        r(1, true, 3, false).span(&eq(2)).unwrap(),
        r(1, true, 3, false)
    );
    assert_eq!(
        r(1, true, 3, false).span(&r(2, false, 10, false)).unwrap(),
        r(1, true, 10, false)
    );
    assert_eq!(gt(1).span(&eq(0)).unwrap(), ge(0));
    assert_eq!(gt(1).span(&ge(10)).unwrap(), gt(1));
    assert_eq!(lt(1).span(&le(1)).unwrap(), le(1));
    assert_eq!(all().span(&le(1)).unwrap(), all());
}

#[test]
fn test_overlaps() {
    let a = BlockAllocatorImpl::new();
    let all = || Range::all(&a, &BIGINT);
    let eq = |v: i64| Range::equal(&a, &BIGINT, v).unwrap();
    let gt = |v: i64| Range::greater_than(&a, &BIGINT, v).unwrap();
    let lt = |v: i64| Range::less_than(&a, &BIGINT, v).unwrap();
    let le = |v: i64| Range::less_than_or_equal(&a, &BIGINT, v).unwrap();
    let r = |l: i64, li: bool, h: i64, hi: bool| Range::range(&a, &BIGINT, l, li, h, hi).unwrap();
    assert!(gt(1).overlaps(&le(2)).unwrap());
    assert!(!gt(2).overlaps(&lt(2)).unwrap());
    assert!(r(1, true, 3, false).overlaps(&eq(2)).unwrap());
    assert!(r(1, true, 3, false)
        .overlaps(&r(2, false, 10, false))
        .unwrap());
    assert!(!r(1, true, 3, false)
        .overlaps(&r(3, true, 10, false))
        .unwrap());
    assert!(r(1, true, 3, true)
        .overlaps(&r(3, true, 10, false))
        .unwrap());
    assert!(all().overlaps(&eq(i64::MAX)).unwrap());
}

#[test]
fn test_intersect() {
    let a = BlockAllocatorImpl::new();
    let all = || Range::all(&a, &BIGINT);
    let eq = |v: i64| Range::equal(&a, &BIGINT, v).unwrap();
    let gt = |v: i64| Range::greater_than(&a, &BIGINT, v).unwrap();
    let le = |v: i64| Range::less_than_or_equal(&a, &BIGINT, v).unwrap();
    let r = |l: i64, li: bool, h: i64, hi: bool| Range::range(&a, &BIGINT, l, li, h, hi).unwrap();
    assert_eq!(gt(1).intersect(&le(2)).unwrap(), r(1, false, 2, true));
    assert_eq!(r(1, true, 3, false).intersect(&eq(2)).unwrap(), eq(2));
    assert_eq!(
        r(1, true, 3, false)
            .intersect(&r(2, false, 10, false))
            .unwrap(),
        r(2, false, 3, false)
    );
    assert_eq!(
        r(1, true, 3, true)
            .intersect(&r(3, true, 10, false))
            .unwrap(),
        eq(3)
    );
    assert_eq!(all().intersect(&eq(i64::MAX)).unwrap(), eq(i64::MAX));
}

#[test]
fn test_exceptional_intersect() {
    let a = BlockAllocatorImpl::new();
    let gt = Range::greater_than(&a, &BIGINT, 2i64).unwrap();
    let lt = Range::less_than(&a, &BIGINT, 2i64).unwrap();
    assert!(is_connector_err(&gt.intersect(&lt)));
    let r1 = Range::range(&a, &BIGINT, 1i64, true, 3i64, false).unwrap();
    let r2 = Range::range(&a, &BIGINT, 3i64, true, 10i64, false).unwrap();
    assert!(is_connector_err(&r1.intersect(&r2)));
}
