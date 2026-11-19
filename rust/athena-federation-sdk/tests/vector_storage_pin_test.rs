use std::fmt::Write as _;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, Schema, TimeUnit};
use bigdecimal::BigDecimal;
use chrono::{NaiveDate, NaiveDateTime};

use athena_federation_sdk::data::*;
use athena_federation_sdk::records::ReadRecordsResponse;
use athena_federation_sdk::request::FederationResponse;
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};

fn list_of(name: &str, item: DataType) -> Field {
    Field::new(
        name,
        DataType::List(Arc::new(Field::new("item", item, true))),
        true,
    )
}

fn schema() -> Schema {
    let fields = vec![
        Field::new("tinyint_col", DataType::Int8, true),
        Field::new("smallint_col", DataType::Int16, true),
        Field::new("int_col", DataType::Int32, true),
        Field::new("bigint_col", DataType::Int64, true),
        Field::new("uint1_col", DataType::UInt8, true),
        Field::new("uint2_col", DataType::UInt16, true),
        Field::new("uint4_col", DataType::UInt32, true),
        Field::new("uint8_col", DataType::UInt64, true),
        Field::new("numeric_scaled", decimal(18, 4), true),
        Field::new("numeric_noscale", decimal(10, 0), true),
        Field::new("numeric_wide", decimal(38, 10), true),
        Field::new("real_col", DataType::Float32, true),
        Field::new("double_col", DataType::Float64, true),
        Field::new("text_col", DataType::Utf8, true),
        Field::new("large_text_col", DataType::LargeUtf8, true),
        Field::new("bool_col", DataType::Boolean, true),
        Field::new("date_col", DataType::Date32, true),
        Field::new("date_milli_col", DataType::Date64, true),
        Field::new(
            "ts_milli_col",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            true,
        ),
        Field::new(
            "ts_micro_col",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            true,
        ),
        Field::new("tstz_col", timestamp_tz(TimeUnit::Millisecond, "UTC"), true),
        Field::new(
            "time_milli_col",
            DataType::Time32(TimeUnit::Millisecond),
            true,
        ),
        Field::new(
            "time_micro_col",
            DataType::Time64(TimeUnit::Microsecond),
            true,
        ),
        Field::new("bytea_col", DataType::Binary, true),
        list_of("int_arr", DataType::Int32),
        list_of("bigint_arr", DataType::Int64),
        list_of("text_arr", DataType::Utf8),
        list_of("float8_arr", DataType::Float64),
        list_of("bool_arr", DataType::Boolean),
        list_of("date_arr", DataType::Date32),
        list_of("numeric_arr", decimal(10, 2)),
        Field::new(
            "struct_col",
            DataType::Struct(Fields::from(vec![
                Field::new("a", DataType::Int32, true),
                Field::new("b", DataType::Utf8, true),
                list_of("c", DataType::Int64),
            ])),
            true,
        ),
        Field::new(
            "map_col",
            map_type(
                false,
                Field::new("key", DataType::Utf8, false),
                Field::new("value", DataType::Int32, true),
            ),
            true,
        ),
    ];
    Schema::new(fields)
}

fn dec(s: &str) -> Value {
    Value::Decimal(BigDecimal::from_str(s).unwrap())
}

fn ldt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32, ms: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .unwrap()
        .and_hms_milli_opt(h, mi, s, ms)
        .unwrap()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn first_row() -> Vec<(&'static str, Value)> {
    vec![
        ("tinyint_col", Value::Byte(-5)),
        ("smallint_col", Value::Short(-16000)),
        ("int_col", Value::Int(123_456)),
        ("bigint_col", Value::Long(-1_500_000_000_123)),
        ("uint1_col", Value::Int(200)),
        ("uint2_col", Value::Int(65_000)),
        ("uint4_col", Value::Int(-5)),
        ("uint8_col", Value::Long(-2)),
        ("numeric_scaled", dec("12.3457")),
        ("numeric_noscale", dec("31")),
        ("numeric_wide", dec("-98765432109876543210.0123456789")),
        ("real_col", Value::Float(0.25)),
        ("double_col", Value::Double(-98.875)),
        ("text_col", s("text_00007 unicode \u{e9}\u{6f22}")),
        ("large_text_col", s("large")),
        ("bool_col", Value::Boolean(true)),
        (
            "date_col",
            Value::LocalDate(NaiveDate::from_ymd_opt(1960, 1, 12).unwrap()),
        ),
        (
            "date_milli_col",
            Value::LocalDateTime(ldt(1965, 6, 16, 11, 37, 11, 123)),
        ),
        (
            "ts_milli_col",
            Value::LocalDateTime(ldt(2001, 2, 3, 4, 5, 6, 789)),
        ),
        ("ts_micro_col", Value::Long(981_173_106_789_123)),
        (
            "tstz_col",
            Value::ZonedDateTime(ZonedDateTime::of_local(
                ldt(2000, 1, 1, 5, 17, 0, 0),
                ZoneId::of("UTC").unwrap(),
            )),
        ),
        ("time_milli_col", Value::Int(37_000)),
        ("time_micro_col", Value::Long(37_000_123)),
        ("bytea_col", Value::Bytes(vec![0, 0, 0, 7, 255])),
        (
            "int_arr",
            Value::List(vec![Value::Int(7), Value::Int(8), Value::Int(9)]),
        ),
        (
            "bigint_arr",
            Value::List(vec![Value::Long(700_000), Value::Long(-7)]),
        ),
        ("text_arr", Value::List(vec![s("a7"), Value::Null, s("b7")])),
        (
            "float8_arr",
            Value::List(vec![Value::Double(3.5), Value::Double(10.5)]),
        ),
        (
            "bool_arr",
            Value::List(vec![Value::Boolean(false), Value::Boolean(true)]),
        ),
        (
            "date_arr",
            Value::List(vec![
                Value::LocalDate(NaiveDate::from_ymd_opt(2020, 1, 8).unwrap()),
                Value::LocalDate(NaiveDate::from_ymd_opt(1969, 12, 24).unwrap()),
            ]),
        ),
        ("numeric_arr", Value::List(vec![dec("8.75"), dec("-5.25")])),
        (
            "struct_col",
            Value::Map(vec![
                (s("a"), Value::Int(1)),
                (s("b"), s("one")),
                (s("c"), Value::List(vec![Value::Long(1), Value::Long(2)])),
            ]),
        ),
        (
            "map_col",
            Value::Map(vec![(s("k1"), Value::Int(1)), (s("k2"), Value::Null)]),
        ),
    ]
}

fn third_row() -> Vec<(&'static str, Value)> {
    vec![
        ("tinyint_col", Value::Byte(127)),
        ("smallint_col", Value::Short(32_000)),
        ("int_col", Value::Int(-1)),
        ("bigint_col", Value::Long(0)),
        ("uint1_col", Value::Int(0)),
        ("uint2_col", Value::Int(1)),
        ("uint4_col", Value::Long(0)),
        ("uint8_col", Value::Long(1)),
        ("numeric_scaled", dec("-0.0001")),
        ("numeric_noscale", dec("0")),
        ("numeric_wide", dec("0.0000000001")),
        ("real_col", Value::Float(-0.0)),
        ("double_col", Value::Double(1e300)),
        ("text_col", s("")),
        ("large_text_col", s("x")),
        ("bool_col", Value::Boolean(false)),
        (
            "date_col",
            Value::LocalDate(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()),
        ),
        (
            "date_milli_col",
            Value::LocalDateTime(ldt(1969, 12, 31, 23, 59, 59, 999)),
        ),
        (
            "ts_milli_col",
            Value::LocalDateTime(ldt(1970, 1, 1, 0, 0, 0, 0)),
        ),
        ("ts_micro_col", Value::Long(-1)),
        (
            "tstz_col",
            Value::ZonedDateTime(ZonedDateTime::of_local(
                ldt(2015, 12, 21, 17, 42, 34, 0),
                ZoneId::of("-05:00").unwrap(),
            )),
        ),
        ("time_milli_col", Value::Int(0)),
        ("time_micro_col", Value::Long(86_399_999_999)),
        ("bytea_col", Value::Bytes(Vec::new())),
        ("int_arr", Value::List(Vec::new())),
        ("bigint_arr", Value::List(vec![Value::Long(i64::MIN)])),
        ("text_arr", Value::List(Vec::new())),
        ("float8_arr", Value::List(vec![Value::Double(-0.5)])),
        ("bool_arr", Value::List(vec![Value::Null])),
        ("date_arr", Value::List(Vec::new())),
        ("numeric_arr", Value::List(vec![dec("0.01")])),
        (
            "struct_col",
            Value::Map(vec![(s("a"), Value::Null), (s("b"), s("two"))]),
        ),
        ("map_col", Value::Map(Vec::new())),
    ]
}

fn row_values(row: usize) -> Vec<(&'static str, Value)> {
    match row {
        0 => first_row(),
        1 => Vec::new(),
        _ => third_row(),
    }
}

fn build_block(allocator: &BlockAllocatorImpl) -> (Block, String) {
    let mut block = allocator.create_block(Arc::new(schema()));
    let mut errors = String::new();
    for row in 0..3 {
        for (name, value) in row_values(row) {
            let vector = block.get_field_vector_mut(name).unwrap();
            let result = if vector.is_complex() {
                block_utils::set_complex_value(vector, row, &DefaultFieldResolver, &value)
            } else {
                block_utils::set_value(vector, row, &value)
            };
            if let Err(e) = result {
                writeln!(errors, "set {name}[{row}]: {e:?}").unwrap();
                let direct = vector.set(row, &value);
                writeln!(errors, "vector.set {name}[{row}]: {direct:?}").unwrap();
            }
        }
    }
    block.set_row_count(3);
    (block, errors)
}

fn describe_vector(out: &mut String, vector: &FieldVector, rows: usize, indent: &str) {
    writeln!(
        out,
        "{indent}{} minor={} value_count={} null_count={} buffer_size={}",
        vector.name(),
        vector.minor_type(),
        vector.value_count(),
        vector.null_count(),
        vector.get_buffer_size()
    )
    .unwrap();
    for i in 0..rows {
        writeln!(
            out,
            "{indent}  [{i}] set={} object={:?} raw={:?}",
            vector.is_set(i),
            vector.get_object(i),
            vector.get_raw(i)
        )
        .unwrap();
    }
    for child in vector.children() {
        let child_rows = child.value_count().max(rows);
        describe_vector(out, child, child_rows, &format!("{indent}    "));
    }
}

fn render(block: &Block) -> String {
    let mut out = String::new();
    writeln!(out, "display: {block}").unwrap();
    for row in 0..block.row_count() {
        writeln!(
            out,
            "row {row}: {:?}",
            block_utils::row_to_string(block, row)
        )
        .unwrap();
    }
    for vector in block.field_vectors() {
        describe_vector(&mut out, vector, block.row_count(), "");
    }
    out
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vector_pin/all_types.txt")
}

#[test]
fn vector_storage_output_is_pinned() {
    let allocator = Arc::new(BlockAllocatorImpl::with_id("vector-pin"));
    let (block, errors) = build_block(&allocator);
    let mut actual = errors;
    actual.push_str(&render(&block));
    let bean_mapper = ObjectMapper::new(SerdeVersion::V1, allocator.clone());
    let json = bean_mapper.write_block(&block).unwrap();
    writeln!(actual, "block json V1: {json}").unwrap();
    let back = bean_mapper.read_block(json.as_bytes()).unwrap();
    assert_eq!(json, bean_mapper.write_block(&back).unwrap());
    assert_eq!(render(&block), render(&back));
    writeln!(actual, "block equals V1 round trip: {}", block == back).unwrap();
    for version in [SerdeVersion::V2, SerdeVersion::V5] {
        let mapper = ObjectMapper::new(version, allocator.clone());
        let response: FederationResponse =
            ReadRecordsResponse::new("catalog", block.clone()).into();
        let json = mapper.write_response(&response).unwrap();
        writeln!(actual, "response json {version:?}: {json}").unwrap();
        let back = mapper.read_response(json.as_bytes()).unwrap();
        assert_eq!(json, mapper.write_response(&back).unwrap());
        let FederationResponse::ReadRecords(back) = back else {
            panic!("unexpected response");
        };
        assert_eq!(render(&block), render(back.records()));
        writeln!(
            actual,
            "block equals {version:?} round trip: {}",
            &block == back.records()
        )
        .unwrap();
    }
    let path = fixture_path();
    if std::env::var_os("UPDATE_VECTOR_PIN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    for (n, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
        assert_eq!(e, a, "line {}", n + 1);
    }
    assert_eq!(expected, actual);
}
