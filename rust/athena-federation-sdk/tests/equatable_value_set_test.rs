use arrow::datatypes::DataType;
use athena_federation_sdk::data::{BlockAllocatorImpl, Value};
use athena_federation_sdk::domain::predicate::{EquatableValueSet, Marker, ValueSet};
use athena_federation_sdk::FederationError;

const INT: DataType = DataType::Int32;

struct H {
    a: BlockAllocatorImpl,
}

impl H {
    fn new() -> H {
        H {
            a: BlockAllocatorImpl::new(),
        }
    }
    fn none(&self) -> EquatableValueSet {
        EquatableValueSet::none(&self.a, &INT)
    }
    fn all(&self) -> EquatableValueSet {
        EquatableValueSet::all(&self.a, &INT)
    }
    fn of(&self, values: &[i32]) -> EquatableValueSet {
        EquatableValueSet::of(&self.a, &INT, values.iter().copied()).unwrap()
    }
    fn not(&self, values: &[i32]) -> EquatableValueSet {
        self.of(values).complement(&self.a).unwrap()
    }
}

fn contains(set: &EquatableValueSet, v: i32) -> bool {
    set.contains_value(&Value::Int(v)).unwrap()
}

#[test]
fn test_empty_set() {
    let h = H::new();
    let equatables = h.none();
    assert_eq!(equatables.get_type(), INT);
    assert!(equatables.is_none());
    assert!(!equatables.is_all());
    assert!(!equatables.is_single_value());
    assert!(equatables.is_allow_list());
    assert_eq!(equatables.values().row_count(), 0);
    assert_eq!(equatables.complement(&h.a).unwrap(), h.all());
    assert!(!contains(&equatables, 0));
    assert!(!contains(&equatables, 1));
}

#[test]
fn test_entire_set() {
    let h = H::new();
    let equatables = h.all();
    assert_eq!(equatables.get_type(), INT);
    assert!(!equatables.is_none());
    assert!(equatables.is_all());
    assert!(!equatables.is_single_value());
    assert!(!equatables.is_allow_list());
    assert_eq!(equatables.values().row_count(), 0);
    assert_eq!(equatables.complement(&h.a).unwrap(), h.none());
    assert!(contains(&equatables, 0));
    assert!(contains(&equatables, 1));
}

#[test]
fn test_single_value() {
    let h = H::new();
    let equatables = h.of(&[10]);
    let complement = h.all().subtract(&h.a, &equatables).unwrap();
    assert_eq!(equatables.get_type(), INT);
    assert!(!equatables.is_none());
    assert!(!equatables.is_all());
    assert!(equatables.is_single_value());
    assert!(equatables.is_allow_list());
    assert_eq!(equatables.get_single_value().unwrap(), Value::Int(10));
    assert_eq!(equatables.complement(&h.a).unwrap(), complement);
    assert!(!contains(&equatables, 0));
    assert!(!contains(&equatables, 1));
    assert!(contains(&equatables, 10));

    assert_eq!(complement.get_type(), INT);
    assert!(!complement.is_none());
    assert!(!complement.is_all());
    assert!(!complement.is_single_value());
    assert!(!complement.is_allow_list());
    assert_eq!(complement.get_value(0), Value::Int(10), "{complement}");
    assert_eq!(complement.complement(&h.a).unwrap(), equatables);
    assert!(contains(&complement, 0));
    assert!(contains(&complement, 1));
    assert!(!contains(&complement, 10));
}

#[test]
fn test_multiple_values() {
    let h = H::new();
    let equatables = h.of(&[1, 2, 3, 1]);
    let complement = h.all().subtract(&h.a, &equatables).unwrap();
    assert_eq!(equatables.get_type(), INT);
    assert!(!equatables.is_none());
    assert!(!equatables.is_all());
    assert!(!equatables.is_single_value());
    assert!(equatables.is_allow_list());
    assert_eq!(equatables.complement(&h.a).unwrap(), complement);
    assert!(!contains(&equatables, 0));
    assert!(contains(&equatables, 1));
    assert!(contains(&equatables, 2));
    assert!(contains(&equatables, 3));
    assert!(!contains(&equatables, 4));

    assert_eq!(complement.get_type(), INT);
    assert!(!complement.is_none());
    assert!(!complement.is_all());
    assert!(!complement.is_single_value());
    assert!(!complement.is_allow_list());
    assert_eq!(complement.complement(&h.a).unwrap(), equatables);
    assert!(contains(&complement, 0));
    assert!(!contains(&complement, 1));
    assert!(!contains(&complement, 2));
    assert!(!contains(&complement, 3));
    assert!(contains(&complement, 4));
}

#[test]
fn test_get_single_value() {
    let h = H::new();
    assert_eq!(h.of(&[0]).get_single_value().unwrap(), Value::Int(0));
    assert!(matches!(
        h.all().get_single_value(),
        Err(FederationError::AthenaConnector(_))
    ));
}

#[test]
fn test_nullability() {
    let h = H::new();
    let actual: ValueSet = EquatableValueSet::of_nullable(&h.a, &INT, true, vec![100])
        .unwrap()
        .into();
    assert!(actual
        .contains_marker(&Marker::exactly(&h.a, &INT, 100).unwrap())
        .unwrap());
    assert!(actual
        .contains_marker(&Marker::null_marker(&h.a, &INT))
        .unwrap());
    assert!(!actual
        .contains_marker(&Marker::exactly(&h.a, &INT, 101).unwrap())
        .unwrap());
}

#[test]
fn test_overlaps() {
    let h = H::new();
    let a = &h.a;
    assert!(h.all().overlaps(a, &h.all()).unwrap());
    assert!(!h.all().overlaps(a, &h.none()).unwrap());
    assert!(h.all().overlaps(a, &h.of(&[0])).unwrap());
    assert!(h.all().overlaps(a, &h.of(&[0, 1])).unwrap());
    assert!(h.all().overlaps(a, &h.not(&[0, 1])).unwrap());

    assert!(!h.none().overlaps(a, &h.all()).unwrap());
    assert!(!h.none().overlaps(a, &h.none()).unwrap());
    assert!(!h.none().overlaps(a, &h.of(&[0])).unwrap());
    assert!(!h.none().overlaps(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.none().overlaps(a, &h.not(&[0, 1])).unwrap());

    assert!(h.of(&[0]).overlaps(a, &h.all()).unwrap());
    assert!(!h.of(&[0]).overlaps(a, &h.none()).unwrap());
    assert!(h.of(&[0]).overlaps(a, &h.of(&[0])).unwrap());
    assert!(!h.of(&[0]).overlaps(a, &h.of(&[1])).unwrap());
    assert!(h.of(&[0]).overlaps(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.of(&[0]).overlaps(a, &h.not(&[0, 1])).unwrap());
    assert!(!h.of(&[0]).overlaps(a, &h.not(&[0])).unwrap());
    assert!(h.of(&[0]).overlaps(a, &h.not(&[1])).unwrap());

    assert!(h.of(&[0, 1]).overlaps(a, &h.all()).unwrap());
    assert!(!h.of(&[0, 1]).overlaps(a, &h.none()).unwrap());
    assert!(h.of(&[0, 1]).overlaps(a, &h.of(&[0])).unwrap());
    assert!(!h.of(&[0, 1]).overlaps(a, &h.of(&[-1])).unwrap());
    assert!(h.of(&[0, 1]).overlaps(a, &h.of(&[0, 1])).unwrap());
    assert!(h.of(&[0, 1]).overlaps(a, &h.not(&[-1])).unwrap());

    assert!(h.not(&[0, 1]).overlaps(a, &h.all()).unwrap());
    assert!(!h.not(&[0, 1]).overlaps(a, &h.none()).unwrap());
    assert!(!h.not(&[0, 1]).overlaps(a, &h.of(&[0])).unwrap());
    assert!(h.not(&[0, 1]).overlaps(a, &h.of(&[-1])).unwrap());
    assert!(!h.not(&[0, 1]).overlaps(a, &h.of(&[0, 1])).unwrap());
    assert!(h.not(&[0, 1]).overlaps(a, &h.not(&[-1])).unwrap());
}

#[test]
fn test_contains() {
    let h = H::new();
    let a = &h.a;
    assert!(h.all().contains(a, &h.all()).unwrap());
    assert!(h.all().contains(a, &h.none()).unwrap());
    assert!(h.all().contains(a, &h.of(&[0])).unwrap());
    assert!(h.all().contains(a, &h.of(&[0, 1])).unwrap());
    assert!(h.all().contains(a, &h.not(&[0, 1])).unwrap());

    assert!(!h.none().contains(a, &h.all()).unwrap());
    assert!(h.none().contains(a, &h.none()).unwrap());
    assert!(!h.none().contains(a, &h.of(&[0])).unwrap());
    assert!(!h.none().contains(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.none().contains(a, &h.not(&[0, 1])).unwrap());

    assert!(!h.of(&[0]).contains(a, &h.all()).unwrap());
    assert!(h.of(&[0]).contains(a, &h.none()).unwrap());
    assert!(h.of(&[0]).contains(a, &h.of(&[0])).unwrap());
    assert!(!h.of(&[0]).contains(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.of(&[0]).contains(a, &h.not(&[0, 1])).unwrap());
    assert!(!h.of(&[0]).contains(a, &h.not(&[0])).unwrap());
    assert!(!h.of(&[0]).contains(a, &h.not(&[1])).unwrap());

    assert!(!h.of(&[0, 1]).contains(a, &h.all()).unwrap());
    assert!(h.of(&[0, 1]).contains(a, &h.none()).unwrap());
    assert!(h.of(&[0, 1]).contains(a, &h.of(&[0])).unwrap());
    assert!(h.of(&[0, 1]).contains(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.of(&[0, 1]).contains(a, &h.of(&[0, 2])).unwrap());
    assert!(!h.of(&[0, 1]).contains(a, &h.not(&[0, 1])).unwrap());
    assert!(!h.of(&[0, 1]).contains(a, &h.not(&[0])).unwrap());
    assert!(!h.of(&[0, 1]).contains(a, &h.not(&[1])).unwrap());

    assert!(!h.not(&[0, 1]).contains(a, &h.all()).unwrap());
    assert!(h.not(&[0, 1]).contains(a, &h.none()).unwrap());
    assert!(!h.not(&[0, 1]).contains(a, &h.of(&[0])).unwrap());
    assert!(h.not(&[0, 1]).contains(a, &h.of(&[-1])).unwrap());
    assert!(!h.not(&[0, 1]).contains(a, &h.of(&[0, 1])).unwrap());
    assert!(!h.not(&[0, 1]).contains(a, &h.not(&[-1])).unwrap());
}

#[test]
fn test_intersect() {
    let h = H::new();
    let a = &h.a;
    assert_eq!(h.none().intersect(a, &h.none()).unwrap(), h.none());
    assert_eq!(h.all().intersect(a, &h.all()).unwrap(), h.all());
    assert_eq!(h.none().intersect(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.none().intersect(a, &h.of(&[0])).unwrap(), h.none());
    assert_eq!(h.all().intersect(a, &h.of(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0]).intersect(a, &h.of(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0, 1]).intersect(a, &h.of(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.not(&[0]).intersect(a, &h.of(&[0])).unwrap(), h.none());
    assert_eq!(h.not(&[0]).intersect(a, &h.of(&[1])).unwrap(), h.of(&[1]));
    assert_eq!(h.of(&[0]).intersect(a, &h.not(&[1])).unwrap(), h.of(&[0]));
    assert_eq!(
        h.of(&[0, 1]).intersect(a, &h.of(&[0, 2])).unwrap(),
        h.of(&[0])
    );
    assert_eq!(
        h.not(&[0, 1]).intersect(a, &h.of(&[0, 2])).unwrap(),
        h.of(&[2])
    );
    assert_eq!(
        h.not(&[0, 1]).intersect(a, &h.not(&[0, 2])).unwrap(),
        h.not(&[0, 1, 2])
    );
}

#[test]
fn test_union() {
    let h = H::new();
    let a = &h.a;
    assert_eq!(h.none().union(a, &h.none()).unwrap(), h.none());
    assert_eq!(h.all().union(a, &h.all()).unwrap(), h.all());
    assert_eq!(h.none().union(a, &h.all()).unwrap(), h.all());
    assert_eq!(h.none().union(a, &h.of(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.all().union(a, &h.of(&[0])).unwrap(), h.all());
    assert_eq!(h.of(&[0]).union(a, &h.of(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0, 1]).union(a, &h.of(&[0])).unwrap(), h.of(&[0, 1]));
    assert_eq!(h.not(&[0]).union(a, &h.of(&[0])).unwrap(), h.all());
    assert_eq!(h.not(&[0]).union(a, &h.of(&[1])).unwrap(), h.not(&[0]));
    assert_eq!(h.of(&[0]).union(a, &h.not(&[1])).unwrap(), h.not(&[1]));
    assert_eq!(
        h.of(&[0, 1]).union(a, &h.of(&[0, 2])).unwrap(),
        h.of(&[0, 1, 2])
    );
    assert_eq!(
        h.not(&[0, 1]).union(a, &h.of(&[0, 2])).unwrap(),
        h.not(&[1])
    );
    assert_eq!(
        h.not(&[0, 1]).union(a, &h.not(&[0, 2])).unwrap(),
        h.not(&[0])
    );
}

#[test]
fn test_subtract() {
    let h = H::new();
    let a = &h.a;
    assert_eq!(h.all().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.all().subtract(a, &h.none()).unwrap(), h.all());
    assert_eq!(h.all().subtract(a, &h.of(&[0])).unwrap(), h.not(&[0]));
    assert_eq!(h.all().subtract(a, &h.of(&[0, 1])).unwrap(), h.not(&[0, 1]));
    assert_eq!(h.all().subtract(a, &h.not(&[0, 1])).unwrap(), h.of(&[0, 1]));

    assert_eq!(h.none().subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.none()).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.of(&[0])).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.of(&[0, 1])).unwrap(), h.none());
    assert_eq!(h.none().subtract(a, &h.not(&[0, 1])).unwrap(), h.none());

    assert_eq!(h.of(&[0]).subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.of(&[0]).subtract(a, &h.none()).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0]).subtract(a, &h.of(&[0])).unwrap(), h.none());
    assert_eq!(h.of(&[0]).subtract(a, &h.not(&[0])).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0]).subtract(a, &h.of(&[1])).unwrap(), h.of(&[0]));
    assert_eq!(h.of(&[0]).subtract(a, &h.not(&[1])).unwrap(), h.none());
    assert_eq!(h.of(&[0]).subtract(a, &h.of(&[0, 1])).unwrap(), h.none());
    assert_eq!(h.of(&[0]).subtract(a, &h.not(&[0, 1])).unwrap(), h.of(&[0]));

    assert_eq!(h.not(&[0]).subtract(a, &h.all()).unwrap(), h.none());
    assert_eq!(h.not(&[0]).subtract(a, &h.none()).unwrap(), h.not(&[0]));
    assert_eq!(h.not(&[0]).subtract(a, &h.of(&[0])).unwrap(), h.not(&[0]));
    assert_eq!(h.not(&[0]).subtract(a, &h.not(&[0])).unwrap(), h.none());
    assert_eq!(
        h.not(&[0]).subtract(a, &h.of(&[1])).unwrap(),
        h.not(&[0, 1])
    );
    assert_eq!(h.not(&[0]).subtract(a, &h.not(&[1])).unwrap(), h.of(&[1]));
    assert_eq!(
        h.not(&[0]).subtract(a, &h.of(&[0, 1])).unwrap(),
        h.not(&[0, 1])
    );
    assert_eq!(
        h.not(&[0]).subtract(a, &h.not(&[0, 1])).unwrap(),
        h.of(&[1])
    );
}
