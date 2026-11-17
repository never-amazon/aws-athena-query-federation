use arrow::datatypes::Field;
use athena_federation_sdk::data::{arrow_type_to_string, get_children, minor_type_of, MinorType};
use athena_federation_sdk::metadata::glue::GlueFieldLexer;
use athena_federation_sdk::FederationError;

fn lex(name: &str, input: &str) -> Field {
    GlueFieldLexer::lex(name, input).unwrap().unwrap()
}

fn minor(field: &Field) -> MinorType {
    minor_type_of(field.data_type())
}

fn children(field: &Field) -> Vec<Field> {
    get_children(field)
        .iter()
        .map(|f| f.as_ref().clone())
        .collect()
}

fn java_field_to_string(field: &Field) -> String {
    let mut text = format!(
        "{}: {}",
        field.name(),
        arrow_type_to_string(field.data_type())
    );
    let kids = children(field);
    if !kids.is_empty() {
        let parts: Vec<String> = kids.iter().map(java_field_to_string).collect();
        text.push('<');
        text.push_str(&parts.join(", "));
        text.push('>');
    }
    if !field.is_nullable() {
        text.push_str(" not null");
    }
    text
}

#[test]
fn basic_lex_test() {
    let field = lex("testField", "ARRAY<STRING>");
    assert_eq!("testField", field.name());
    assert_eq!(MinorType::LIST, minor(&field));
    assert_eq!(MinorType::VARCHAR, minor(&children(&field)[0]));
}

#[test]
fn base_lex_test() {
    let field = lex("testField", "INT");
    assert_eq!("testField", field.name());
    assert_eq!(MinorType::INT, minor(&field));
    assert_eq!(0, children(&field).len());
}

#[test]
fn lex_test() {
    let input = "STRUCT <  street_address: STRUCT <    street_number: INT,    street_name: STRING,    street_type: STRING  >,  country: STRING,  postal_code: ARRAY<STRING>>";
    let field = lex("testField", input);
    assert_eq!("testField", field.name());
    assert_eq!(MinorType::STRUCT, minor(&field));
    let level1 = children(&field);
    assert_eq!(3, level1.len());
    assert_eq!("street_address", level1[0].name());
    assert_eq!(MinorType::STRUCT, minor(&level1[0]));
    let level2 = children(&level1[0]);
    assert_eq!(3, level2.len());
    assert_eq!("street_number", level2[0].name());
    assert_eq!(MinorType::INT, minor(&level2[0]));
    assert_eq!(0, children(&level2[0]).len());
    assert_eq!("street_name", level2[1].name());
    assert_eq!(MinorType::VARCHAR, minor(&level2[1]));
    assert_eq!(0, children(&level2[1]).len());
    assert_eq!("street_type", level2[2].name());
    assert_eq!(MinorType::VARCHAR, minor(&level2[2]));
    assert_eq!(0, children(&level2[2]).len());
    assert_eq!("country", level1[1].name());
    assert_eq!(MinorType::VARCHAR, minor(&level1[1]));
    assert_eq!(0, children(&level1[1]).len());
    assert_eq!("postal_code", level1[2].name());
    assert_eq!(MinorType::LIST, minor(&level1[2]));
    assert_eq!(1, children(&level1[2]).len());
    assert_eq!(MinorType::VARCHAR, minor(&children(&level1[2])[0]));
}

#[test]
fn array_of_struct_lex_complex_test() {
    let field = lex(
        "namelist",
        "ARRAY<STRUCT<last:STRING,mi:STRING,first:STRING>>",
    );
    assert_eq!("namelist", field.name());
    assert_eq!(MinorType::LIST, minor(&field));
    let child = &children(&field)[0];
    assert_eq!(MinorType::STRUCT, minor(child));
    let kids = children(child);
    assert_eq!(3, kids.len());
    for (kid, name) in kids.iter().zip(["last", "mi", "first"]) {
        assert_eq!(name, kid.name());
        assert_eq!(MinorType::VARCHAR, minor(kid));
    }
}

#[test]
fn nested_array_lex_complex_test() {
    let field = lex(
        "namelist",
        "ARRAY<ARRAY<STRUCT<last:STRING,mi:STRING,first:STRING, aliases:ARRAY<STRING>>>>",
    );
    assert_eq!("namelist", field.name());
    assert_eq!(MinorType::LIST, minor(&field));
    let level1 = &children(&field)[0];
    assert_eq!(MinorType::LIST, minor(level1));
    let level2 = &children(level1)[0];
    assert_eq!(MinorType::STRUCT, minor(level2));
    let kids = children(level2);
    assert_eq!(4, kids.len());
    for (kid, name) in kids.iter().zip(["last", "mi", "first"]) {
        assert_eq!(name, kid.name());
        assert_eq!(MinorType::VARCHAR, minor(kid));
    }
    assert_eq!("aliases", kids[3].name());
    let level3 = &kids[3];
    assert_eq!(MinorType::LIST, minor(level3));
    assert_eq!("aliases", children(level3)[0].name());
    assert_eq!(MinorType::VARCHAR, minor(&children(level3)[0]));
}

#[test]
fn multi_array_struct_lex_complex_test() {
    let field = lex(
        "movie_info",
        "STRUCT<actors:ARRAY<STRING>,genre:ARRAY<STRING>>",
    );
    assert_eq!(MinorType::STRUCT, minor(&field));
    assert_eq!("movie_info", field.name());
    let kids = children(&field);
    assert_eq!(2, kids.len());
    assert_eq!(MinorType::LIST, minor(&kids[0]));
    assert_eq!("actors", children(&kids[0])[0].name());
    assert_eq!(MinorType::LIST, minor(&kids[1]));
    assert_eq!("genre", children(&kids[1])[0].name());
}

#[test]
fn lex_list_of_struct_test() {
    let field = lex(
        "testField",
        "ARRAY<STRUCT<time:timestamp, measure_value\\:\\:double:double>>",
    );
    assert_eq!("testField", field.name());
    assert_eq!(MinorType::LIST, minor(&field));
    let level1 = children(&field);
    assert_eq!(1, level1.len());
    assert_eq!("testField", level1[0].name());
    assert_eq!(MinorType::STRUCT, minor(&level1[0]));
    let level2 = children(&level1[0]);
    assert_eq!(2, level2.len());
    assert_eq!("time", level2[0].name());
    assert_eq!(MinorType::DATEMILLI, minor(&level2[0]));
    assert_eq!(0, children(&level2[0]).len());
    assert_eq!("measure_value::double", level2[1].name());
    assert_eq!(MinorType::FLOAT8, minor(&level2[1]));
}

#[test]
fn lex_struct_list_child_and_struct_child_test() {
    let input =
        "struct<somearrfield:array<set<string>>,mapinner:struct<numberset_deep:set<bigint>>>";
    let field = lex("testAsdf", input);
    assert_eq!("testAsdf", field.name());
    assert_eq!(MinorType::STRUCT, minor(&field));
    let level1 = children(&field);
    assert_eq!(2, level1.len());
    assert_eq!("somearrfield", level1[0].name());
    assert_eq!(MinorType::LIST, minor(&level1[0]));
    let inner = &children(&level1[0])[0];
    assert_eq!(MinorType::LIST, minor(inner));
    assert_eq!(MinorType::VARCHAR, minor(&children(inner)[0]));
    assert_eq!("mapinner", level1[1].name());
    assert_eq!(MinorType::STRUCT, minor(&level1[1]));
    let deep = &children(&level1[1])[0];
    assert_eq!(MinorType::LIST, minor(deep));
    assert_eq!(MinorType::BIGINT, minor(&children(deep)[0]));
}

#[test]
fn lex_extra_inner_closing_fields_test() {
    let input = concat!(
        "struct<",
        "somearrfield0:array<set<struct<somefield:string,someset:set<string>>>>,",
        "mapinner0:struct<numberset_deep:set<bigint>>,",
        "somearrfield1:array<set<struct<somefield:string,someset:set<string>>>>,",
        "mapinner1:struct<numberset_deep:set<bigint>>,",
        "somearrfield2:array<set<struct<somefield:string,someset:set<string>>>>,",
        "mapinner2:struct<numberset_deep:set<bigint>> >"
    );
    let field = lex("testAsdf2", input);
    assert_eq!("testAsdf2", field.name());
    assert_eq!(MinorType::STRUCT, minor(&field));
    let level1 = children(&field);
    assert_eq!(6, level1.len());
    for i in 0..3 {
        let somearr_field = &level1[i * 2];
        assert_eq!(format!("somearrfield{i}"), *somearr_field.name());
        assert_eq!(MinorType::LIST, minor(somearr_field));
        let set = &children(somearr_field)[0];
        assert_eq!(MinorType::LIST, minor(set));
        let inner_struct = &children(set)[0];
        assert_eq!(MinorType::STRUCT, minor(inner_struct));
        let inner = children(inner_struct);
        assert_eq!(MinorType::VARCHAR, minor(&inner[0]));
        assert_eq!(MinorType::LIST, minor(&inner[1]));
        assert_eq!(MinorType::VARCHAR, minor(&children(&inner[1])[0]));
        let mapinner_field = &level1[i * 2 + 1];
        assert_eq!(format!("mapinner{i}"), *mapinner_field.name());
        assert_eq!(MinorType::STRUCT, minor(mapinner_field));
        let deep = &children(mapinner_field)[0];
        assert_eq!(MinorType::LIST, minor(deep));
        assert_eq!(MinorType::BIGINT, minor(&children(deep)[0]));
    }
}

#[test]
fn basic_lex_decimal_test() {
    let field1 = lex("testField1", "DECIMAL(13,7)");
    assert_eq!(
        "testField1: Decimal(13, 7, 128)",
        java_field_to_string(&field1)
    );
    let field2 = lex("testField2", "DECIMAL(13,7,32)");
    assert_eq!(
        "testField2: Decimal(13, 7, 32)",
        java_field_to_string(&field2)
    );
    let err = GlueFieldLexer::lex("testField2", "DECIMAL(13,7,8)").unwrap_err();
    assert_unsupported_decimal_width(&err);
    let field3 = lex("testField3", "DECIMAL");
    assert_eq!(
        "testField3: Decimal(38, 18, 128)",
        java_field_to_string(&field3)
    );
}

#[test]
fn lex_extra_inner_closing_fields_decimals_test() {
    let input = concat!(
        "struct<",
        "somearrfield0:array<set<struct<somefield:decimal(38,9),someset:set<decimal(11,7)>>>>,",
        "mapinner0:struct<numberset_deep:set<decimal(23,3,32)>>,",
        "somearrfield1:array<set<struct<somefield:decimal(38,9),someset:set<decimal(11,7)>>>>,",
        "mapinner1:struct<numberset_deep:set<decimal(23,3,64)>>,",
        "somearrfield2:array<set<struct<somefield:decimal(38,9),someset:set<decimal(11,7)>>>>,",
        "mapinner2:struct<numberset_deep:set<decimal(23,3,256)>>>"
    );
    let field = lex("testAsdf2", input);
    let expected_field_to_string = concat!(
        "testAsdf2: ",
        "Struct<somearrfield0: List<somearrfield0: List<somearrfield0: Struct<somefield: Decimal(38, 9, 128), someset: List<someset: Decimal(11, 7, 128)>>>>, mapinner0: Struct<numberset_deep: List<numberset_deep: Decimal(23, 3, 32)>>, ",
        "somearrfield1: List<somearrfield1: List<somearrfield1: Struct<somefield: Decimal(38, 9, 128), someset: List<someset: Decimal(11, 7, 128)>>>>, mapinner1: Struct<numberset_deep: List<numberset_deep: Decimal(23, 3, 64)>>, ",
        "somearrfield2: List<somearrfield2: List<somearrfield2: Struct<somefield: Decimal(38, 9, 128), someset: List<someset: Decimal(11, 7, 128)>>>>, mapinner2: Struct<numberset_deep: List<numberset_deep: Decimal(23, 3, 256)>>>"
    );
    assert_eq!(expected_field_to_string, java_field_to_string(&field));
    let java_widths = "struct<mapinner0:struct<numberset_deep:set<decimal(23,3,8)>>>";
    assert_unsupported_decimal_width(&GlueFieldLexer::lex("testAsdf2", java_widths).unwrap_err());
}

fn assert_unsupported_decimal_width(err: &FederationError) {
    assert_eq!("java.lang.IllegalArgumentException", err.java_class_name());
    assert_eq!(
        "Library only supports 128-bit and 256-bit decimal values",
        err.to_string()
    );
}

#[test]
fn lex_map_test() {
    if GlueFieldLexer::MAP_DISABLED {
        return;
    }
    let field = lex("SomeMap", "map<array<int>, map<string, string>>");
    let expected_field_to_string = "SomeMap: Map(false)<entries: Struct<key: List<key: Int(32, true)> not null, value: Map(false)<entries: Struct<key: Utf8 not null, value: Utf8> not null>> not null>";
    assert_eq!(expected_field_to_string, java_field_to_string(&field));

    let inner_struct = concat!(
        "struct<",
        "somearrfield0:array<set<struct<somefield:decimal(38,9),someset:set<decimal(11,7)>>>>,",
        "mapinner0:struct<numberset_deep:set<decimal(23,3,64)>>",
        ">"
    );
    let input2 = format!("map<{inner_struct}, map<string, {inner_struct}>>");
    let field2 = lex("SomeMap2", &input2);
    let expected_field_to_string2 = "SomeMap2: Map(false)<entries: Struct<key: Struct<somearrfield0: List<somearrfield0: List<somearrfield0: Struct<somefield: Decimal(38, 9, 128), someset: List<someset: Decimal(11, 7, 128)>>>>, mapinner0: Struct<numberset_deep: List<numberset_deep: Decimal(23, 3, 64)>>> not null, value: Map(false)<entries: Struct<key: Utf8 not null, value: Struct<somearrfield0: List<somearrfield0: List<somearrfield0: Struct<somefield: Decimal(38, 9, 128), someset: List<someset: Decimal(11, 7, 128)>>>>, mapinner0: Struct<numberset_deep: List<numberset_deep: Decimal(23, 3, 64)>>>> not null>> not null>";
    assert_eq!(expected_field_to_string2, java_field_to_string(&field2));
}

#[test]
fn lex_treats_first_character_as_escaped_like_java() {
    assert_eq!(None, GlueFieldLexer::lex("col", "<int>").unwrap());
}

#[test]
fn lex_map_without_value_type_reports_java_null_pointer() {
    for input in ["map<string>", "map<int,unknowntype>"] {
        let err = GlueFieldLexer::lex("col", input).unwrap_err();
        assert_eq!("java.lang.NullPointerException", err.java_class_name());
        assert_eq!(
            "Cannot invoke \"org.apache.arrow.vector.types.pojo.Field.getField(com.google.flatbuffers.FlatBufferBuilder)\" because the return value of \"java.util.List.get(int)\" is null",
            err.to_string(),
            "{input}"
        );
    }
}

#[test]
fn lex_decimal_widths_java_accepts_and_arrow_rs_cannot_hold() {
    for input in ["decimal(10,2,3)", "decimal(10,2,8)", "decimal(300,2,8)"] {
        assert_unsupported_decimal_width(&GlueFieldLexer::lex("col", input).unwrap_err());
    }
    for input in ["decimal(300,2)", "decimal(10,200)"] {
        let err = GlueFieldLexer::lex("col", input).unwrap_err();
        assert_eq!(
            format!(
                "Unsupported decimal type {input}: arrow-rs supports precision up to 255 and scale up to 127"
            ),
            err.to_string()
        );
    }
    assert_eq!(
        "col: Decimal(255, 127, 128)",
        java_field_to_string(&lex("col", "decimal(255,127)"))
    );
}
