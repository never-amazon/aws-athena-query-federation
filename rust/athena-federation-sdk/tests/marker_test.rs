use arrow::datatypes::DataType;
use athena_federation_sdk::data::BlockAllocatorImpl;
use athena_federation_sdk::domain::predicate::Marker;
use athena_federation_sdk::FederationError;

const INT: DataType = DataType::Int32;

fn is_connector_err<T>(r: Result<T, FederationError>) -> bool {
    matches!(r, Err(FederationError::AthenaConnector(_)))
}

#[test]
fn test_types() {
    let a = BlockAllocatorImpl::new();
    assert_eq!(Marker::lower_unbounded(&a, &INT).get_type(), &INT);
    assert_eq!(Marker::below(&a, &INT, 1).unwrap().get_type(), &INT);
    assert_eq!(Marker::exactly(&a, &INT, 1).unwrap().get_type(), &INT);
    assert_eq!(Marker::above(&a, &INT, 1).unwrap().get_type(), &INT);
    assert_eq!(Marker::upper_unbounded(&a, &INT).get_type(), &INT);
}

#[test]
fn test_unbounded() {
    let a = BlockAllocatorImpl::new();
    assert!(Marker::lower_unbounded(&a, &INT).is_lower_unbounded());
    assert!(!Marker::lower_unbounded(&a, &INT).is_upper_unbounded());
    assert!(Marker::upper_unbounded(&a, &INT).is_upper_unbounded());
    assert!(!Marker::upper_unbounded(&a, &INT).is_lower_unbounded());
    assert!(!Marker::below(&a, &INT, 1).unwrap().is_lower_unbounded());
    assert!(!Marker::below(&a, &INT, 1).unwrap().is_upper_unbounded());
    assert!(!Marker::exactly(&a, &INT, 1).unwrap().is_lower_unbounded());
    assert!(!Marker::exactly(&a, &INT, 1).unwrap().is_upper_unbounded());
    assert!(!Marker::above(&a, &INT, 1).unwrap().is_lower_unbounded());
    assert!(!Marker::above(&a, &INT, 1).unwrap().is_upper_unbounded());
}

fn ordered_markers(a: &BlockAllocatorImpl) -> Vec<Marker> {
    vec![
        Marker::lower_unbounded(a, &INT),
        Marker::above(a, &INT, 0).unwrap(),
        Marker::below(a, &INT, 1).unwrap(),
        Marker::exactly(a, &INT, 1).unwrap(),
        Marker::above(a, &INT, 1).unwrap(),
        Marker::below(a, &INT, 2).unwrap(),
        Marker::upper_unbounded(a, &INT),
    ]
}

#[test]
fn test_comparisons() {
    let a = BlockAllocatorImpl::new();
    let markers = ordered_markers(&a);
    for w in markers.windows(2) {
        assert!(w[0].compare_to(&w[1]).unwrap() < 0);
    }
    for i in 0..markers.len() {
        for j in 0..markers.len() {
            assert_eq!(markers[i].compare_to(&markers[j]).unwrap(), (i as i32).cmp(&(j as i32)) as i32);
        }
    }
}

#[test]
fn test_adjacency() {
    let a = BlockAllocatorImpl::new();
    let markers: Vec<(Marker, i32)> = ordered_markers(&a)
        .into_iter()
        .zip([-1000, -100, -1, 0, 1, 100, 1000])
        .collect();
    for (m1, v1) in &markers {
        for (m2, v2) in &markers {
            let adjacent = m1.is_adjacent(m2).unwrap();
            let distance_is_one = (v1 - v2).abs() == 1;
            assert_eq!(adjacent, distance_is_one);
        }
    }
    assert_eq!(Marker::below(&a, &INT, 1).unwrap().greater_adjacent().unwrap(), Marker::exactly(&a, &INT, 1).unwrap());
    assert_eq!(Marker::exactly(&a, &INT, 1).unwrap().greater_adjacent().unwrap(), Marker::above(&a, &INT, 1).unwrap());
    assert_eq!(Marker::above(&a, &INT, 1).unwrap().lesser_adjacent().unwrap(), Marker::exactly(&a, &INT, 1).unwrap());
    assert_eq!(Marker::exactly(&a, &INT, 1).unwrap().lesser_adjacent().unwrap(), Marker::below(&a, &INT, 1).unwrap());
    assert!(is_connector_err(Marker::below(&a, &INT, 1).unwrap().lesser_adjacent()));
    assert!(is_connector_err(Marker::above(&a, &INT, 1).unwrap().greater_adjacent()));
    assert!(is_connector_err(Marker::lower_unbounded(&a, &INT).lesser_adjacent()));
    assert!(is_connector_err(Marker::lower_unbounded(&a, &INT).greater_adjacent()));
    assert!(is_connector_err(Marker::upper_unbounded(&a, &INT).lesser_adjacent()));
    assert!(is_connector_err(Marker::upper_unbounded(&a, &INT).greater_adjacent()));
}
