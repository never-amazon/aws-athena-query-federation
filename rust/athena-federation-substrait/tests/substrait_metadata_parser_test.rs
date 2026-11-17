use athena_federation_substrait::proto::r#type::{self, Kind};
use athena_federation_substrait::proto::{NamedStruct, ReadRel, Type};
use athena_federation_substrait::SubstraitMetadataParser;
use athena_federation_substrait::SubstraitRelModel;

fn create_rel_model(names: &[&str], types: Vec<Type>) -> SubstraitRelModel {
    let named_struct = NamedStruct {
        names: names.iter().map(std::string::ToString::to_string).collect(),
        r#struct: Some(r#type::Struct {
            types,
            ..Default::default()
        }),
    };
    let read_rel = ReadRel {
        base_schema: Some(named_struct),
        ..Default::default()
    };
    SubstraitRelModel::new(Some(read_rel), None, None, None, None)
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
    let model = create_rel_model(
        &["id", "name", "age"],
        vec![create_int_type(), create_string_type(), create_int_type()],
    );
    let columns = SubstraitMetadataParser::get_table_columns(&model).unwrap();
    assert_eq!(3, columns.len());
    assert_eq!("id", columns[0]);
    assert_eq!("name", columns[1]);
    assert_eq!("age", columns[2]);
}

#[test]
fn test_get_table_columns_with_struct_type() {
    let model = create_rel_model(
        &["id", "address", "street", "city", "name"],
        vec![
            create_int_type(),
            create_struct_type(vec![create_string_type(), create_string_type()]),
            create_string_type(),
        ],
    );
    let columns = SubstraitMetadataParser::get_table_columns(&model).unwrap();
    assert_eq!(3, columns.len());
    assert_eq!("id", columns[0]);
    assert_eq!("address", columns[1]);
    assert_eq!("name", columns[2]);
}

#[test]
fn test_get_table_columns_with_empty_schema() {
    let model = create_rel_model(&[], vec![]);
    let columns = SubstraitMetadataParser::get_table_columns(&model).unwrap();
    assert!(columns.is_empty());
}

#[test]
fn test_get_table_columns_with_single_column() {
    let model = create_rel_model(&["single_column"], vec![create_string_type()]);
    let columns = SubstraitMetadataParser::get_table_columns(&model).unwrap();
    assert_eq!(1, columns.len());
    assert_eq!("single_column", columns[0]);
}
