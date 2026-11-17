use arrow::datatypes::DataType;
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{AllOrNoneValueSet, Marker, ValueSet};

const INT: DataType = DataType::Int32;

#[test]
fn test_all() {
    let a = BlockAllocatorImpl::new();
    let value_set = AllOrNoneValueSet::all(&INT);
    assert_eq!(value_set.get_type(), &INT);
    assert!(!value_set.is_none());
    assert!(value_set.is_all());
    assert!(!value_set.is_single_value());
    assert!(value_set
        .contains_marker(&Marker::exactly(&a, &INT, 0).unwrap())
        .unwrap());
    assert!(value_set.get_single_value().is_err());
}

#[test]
fn test_nullability() {
    let a = BlockAllocatorImpl::new();
    let not_null: ValueSet = AllOrNoneValueSet::not_null(&INT).into();
    assert!(not_null
        .contains_marker(&Marker::exactly(&a, &INT, 100).unwrap())
        .unwrap());
    assert!(!not_null
        .contains_marker(&Marker::null_marker(&a, &INT))
        .unwrap());
    assert!(not_null
        .contains_marker(&Marker::exactly(&a, &INT, 101).unwrap())
        .unwrap());
    let only_null: ValueSet = AllOrNoneValueSet::only_null(&INT).into();
    assert!(!only_null
        .contains_marker(&Marker::exactly(&a, &INT, 100).unwrap())
        .unwrap());
    assert!(only_null
        .contains_marker(&Marker::null_marker(&a, &INT))
        .unwrap());
    assert!(!only_null
        .contains_marker(&Marker::exactly(&a, &INT, 101).unwrap())
        .unwrap());
    assert!(only_null.contains_value(&Value::Null).unwrap());
}

#[test]
fn test_none() {
    let a = BlockAllocatorImpl::new();
    let value_set = AllOrNoneValueSet::none(&INT);
    assert_eq!(value_set.get_type(), &INT);
    assert!(value_set.is_none());
    assert!(!value_set.is_all());
    assert!(!value_set.is_single_value());
    assert!(!value_set
        .contains_marker(&Marker::exactly(&a, &INT, 0).unwrap())
        .unwrap());
    assert!(value_set.get_single_value().is_err());
}

#[test]
fn test_intersect() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert_eq!(all.intersect(&a, &all).unwrap(), all);
    assert_eq!(all.intersect(&a, &none).unwrap(), none);
    assert_eq!(none.intersect(&a, &all).unwrap(), none);
    assert_eq!(none.intersect(&a, &none).unwrap(), none);
}

#[test]
fn test_union() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert_eq!(all.union(&a, &all).unwrap(), all);
    assert_eq!(all.union(&a, &none).unwrap(), all);
    assert_eq!(none.union(&a, &all).unwrap(), all);
    assert_eq!(none.union(&a, &none).unwrap(), none);
}

#[test]
fn test_complement() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert_eq!(all.complement(&a).unwrap(), none);
    assert_eq!(none.complement(&a).unwrap(), all);
}

#[test]
fn test_overlaps() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert!(all.overlaps(&a, &all).unwrap());
    assert!(!all.overlaps(&a, &none).unwrap());
    assert!(!none.overlaps(&a, &all).unwrap());
    assert!(!none.overlaps(&a, &none).unwrap());
}

#[test]
fn test_subtract() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert_eq!(all.subtract(&a, &all).unwrap(), none);
    assert_eq!(all.subtract(&a, &none).unwrap(), all);
    assert_eq!(none.subtract(&a, &all).unwrap(), none);
    assert_eq!(none.subtract(&a, &none).unwrap(), none);
}

#[test]
fn test_contains() {
    let a = BlockAllocatorImpl::new();
    let all = AllOrNoneValueSet::all(&INT);
    let none = AllOrNoneValueSet::none(&INT);
    assert!(all.contains(&a, &all).unwrap());
    assert!(all.contains(&a, &none).unwrap());
    assert!(!none.contains(&a, &all).unwrap());
    assert!(none.contains(&a, &none).unwrap());
}
