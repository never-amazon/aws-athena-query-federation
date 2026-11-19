use athena_federation_substrait::plan;
use athena_federation_substrait::proto::r#type::{self, Kind};
use athena_federation_substrait::proto::{NamedStruct, ReadRel, Type};

fn create_read_rel(names: &[&str], types: Vec<Type>) -> ReadRel {
    let named_struct = NamedStruct {
        names: names.iter().map(std::string::ToString::to_string).collect(),
        r#struct: Some(r#type::Struct {
            types,
            ..Default::default()
        }),
    };
    ReadRel {
        base_schema: Some(named_struct),
        ..Default::default()
    }
}

fn create_int_type() -> Type {
    Type {
        kind: Some(Kind::I32(r#type::I32::default())),
    }
}

fn create_string_type() -> Type {
    Type {
        kind: Some(Kind::String(r#type::String::default())),
    }
}

fn create_struct_type(child_types: Vec<Type>) -> Type {
    Type {
        kind: Some(Kind::Struct(r#type::Struct {
            types: child_types,
            ..Default::default()
        })),
    }
}

#[test]
fn test_get_table_columns_with_simple_types() {
    let read_rel = create_read_rel(
        &["id", "name", "age"],
        vec![create_int_type(), create_string_type(), create_int_type()],
    );
    let columns = plan::table_columns(&read_rel).unwrap();
    assert_eq!(3, columns.len());
    assert_eq!("id", columns[0]);
    assert_eq!("name", columns[1]);
    assert_eq!("age", columns[2]);
}

#[test]
fn test_get_table_columns_with_struct_type() {
    let read_rel = create_read_rel(
        &["id", "address", "street", "city", "name"],
        vec![
            create_int_type(),
            create_struct_type(vec![create_string_type(), create_string_type()]),
            create_string_type(),
        ],
    );
    let columns = plan::table_columns(&read_rel).unwrap();
    assert_eq!(3, columns.len());
    assert_eq!("id", columns[0]);
    assert_eq!("address", columns[1]);
    assert_eq!("name", columns[2]);
}

#[test]
fn test_get_table_columns_with_empty_schema() {
    let read_rel = create_read_rel(&[], vec![]);
    let columns = plan::table_columns(&read_rel).unwrap();
    assert!(columns.is_empty());
}

#[test]
fn test_get_table_columns_with_single_column() {
    let read_rel = create_read_rel(&["single_column"], vec![create_string_type()]);
    let columns = plan::table_columns(&read_rel).unwrap();
    assert_eq!(1, columns.len());
    assert_eq!("single_column", columns[0]);
}
