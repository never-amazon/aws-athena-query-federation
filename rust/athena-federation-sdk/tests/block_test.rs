use std::collections::HashMap;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, FieldRef, Fields, Schema};
use base64::Engine;
use bigdecimal::{BigDecimal, FromPrimitive, RoundingMode};
use indexmap::IndexMap;

use athena_federation_sdk::data::field_resolver::DEFAULT;
use athena_federation_sdk::data::types::{decimal, get_children, minor_type_of};
use athena_federation_sdk::data::value::OpaqueValue;
use athena_federation_sdk::data::*;
use athena_federation_sdk::domain::predicate::{ConstraintEvaluator, Constraints, EquatableValueSet, ValueSet, DEFAULT_NO_LIMIT};
use athena_federation_sdk::exceptions::{FederationError, Result};

const ENTRIES: &str = "entries";
const KEY: &str = "key";
const VALUE: &str = "value";

fn assert_serialization_overhead(serialized_block: &[u8]) {
    let encoded = base64::engine::general_purpose::STANDARD.encode(serialized_block);
    let json_size = "{\"field\":\"\"}".len() + encoded.len();
    let overhead = 1.0 - (serialized_block.len() as f64) / (json_size as f64);
    assert!(0.35 > overhead);
}

fn round_trip(expected_block: &Block, orig_schema: &Schema) -> Block {
    let expected_allocator = BlockAllocatorImpl::new();
    let expect_serde = RecordBatchSerDe::new(&expected_allocator);
    let mut block_out = Vec::new();
    let expected_batch = expected_block.get_arrow_record_batch().unwrap();
    expect_serde.serialize(&expected_batch, &mut block_out).unwrap();
    assert_serialization_overhead(&block_out);

    let mut schema_out = Vec::new();
    let schema_serde = SchemaSerDe::new();
    schema_serde.serialize(orig_schema, &mut schema_out).unwrap();
    let actual_schema = schema_serde.deserialize(&mut schema_out.as_slice()).unwrap();

    let actual_allocator = BlockAllocatorImpl::new();
    let actual_serde = RecordBatchSerDe::new(&actual_allocator);
    let batch = actual_serde.deserialize(&block_out).unwrap();
    let mut actual_block = actual_allocator.create_block(Arc::new(actual_schema));
    actual_block.load_arrow_record_batch(&batch).unwrap();
    actual_block
}

#[test]
fn constrained_block_test() {
    let allocator = BlockAllocatorImpl::new();
    let schema = SchemaBuilder::new_builder()
        .add_int_field("col1")
        .add_int_field("col2")
        .build();
    let mut block = allocator.create_block(Arc::new(schema.clone()));
    let col1_constraint: ValueSet = EquatableValueSet::new_builder(&allocator, &DataType::Int32, true, false)
        .add(10)
        .build()
        .unwrap()
        .into();
    let mut summary = IndexMap::new();
    summary.insert("col1".to_string(), col1_constraint);
    let constraints = Constraints::new(summary, Vec::new(), Vec::new(), DEFAULT_NO_LIMIT, HashMap::new(), None);
    let constraint_evaluator = Arc::new(ConstraintEvaluator::new(Some(&allocator), &schema, constraints));
    block.constrain(Some(constraint_evaluator));
    assert!(block.set_value("col1", 0, 10).unwrap());
    assert!(block.offer_value("col1", 0, 10).unwrap());
    assert!(!block.set_value("col1", 0, 11).unwrap());
    assert!(!block.offer_value("col1", 0, 11).unwrap());
    assert!(block.offer_value("unkown_col", 0, 10).unwrap());
}

#[test]
fn end_to_end_block_test() {
    let expected_allocator = BlockAllocatorImpl::new();
    let expected_rows = 200;
    let orig_schema = generate_test_schema();
    let expected_block = generate_test_block(&expected_allocator, &orig_schema, expected_rows);

    let mut actual_block = round_trip(&expected_block, &orig_schema);
    for i in 0..actual_block.get_row_count() {
        BlockUtils::row_to_string(&actual_block, i).unwrap();
    }
    assert_eq!(expected_rows, actual_block.get_row_count(), "Row count missmatch");

    for (actual_field_count, next) in (1usize..).zip(actual_block.get_fields()) {
        let vector = actual_block.get_field_reader(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::INT => {
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    assert_eq!((i * actual_field_count * 3) as i32, vector.read_integer().unwrap());
                }
            }
            MinorType::FLOAT8 => {
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    let expected = i as f64 * actual_field_count as f64 * 1.1;
                    assert!((expected - vector.read_double().unwrap()).abs() <= 0.1);
                }
            }
            MinorType::VARCHAR => {
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    assert_eq!((i * actual_field_count).to_string(), vector.read_text().unwrap());
                }
            }
            MinorType::STRUCT => {
                for i in 0..actual_block.get_row_count() {
                    let effective_vector = if vector.get_field().name() == "structFieldNested28" {
                        vector.reader_named("nestedStruct").unwrap()
                    } else {
                        vector.clone()
                    };
                    effective_vector.set_position(i);
                    assert_eq!(
                        Some(i as i64),
                        effective_vector.reader_named("nestedBigInt").unwrap().read_long(),
                        " name: {}",
                        effective_vector.get_field().name()
                    );
                    assert_eq!(
                        (1000 + i).to_string(),
                        effective_vector.reader_named("nestedString").unwrap().read_text().unwrap()
                    );
                }
            }
            MinorType::LIST => {
                let mut actual = 0;
                let child = get_children(vector.get_field())[0].clone();
                let child_type = minor_type_of(child.data_type());
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    let mut entry_values = 0usize;
                    while vector.next() {
                        let reader = vector.reader().unwrap();
                        match child_type {
                            MinorType::BIGINT => {
                                assert_eq!(Some((i + entry_values) as i64), reader.read_long());
                                entry_values += 1;
                            }
                            MinorType::VARCHAR => {
                                assert_eq!((1000 + i + entry_values).to_string(), reader.read_text().unwrap());
                                entry_values += 1;
                            }
                            MinorType::SMALLINT => {
                                entry_values += 1;
                                assert_eq!((i + 1) as i16, reader.read_short().unwrap());
                            }
                            MinorType::INT => {
                                entry_values += 1;
                                assert_eq!(i as i32, reader.read_integer().unwrap());
                            }
                            MinorType::TINYINT => {
                                entry_values += 1;
                                assert_eq!(i as i8, reader.read_byte().unwrap());
                            }
                            MinorType::FLOAT4 => {
                                entry_values += 1;
                                assert_eq!(i as f32 * 1.0f32, reader.read_float().unwrap());
                            }
                            MinorType::FLOAT8 => {
                                entry_values += 1;
                                assert_eq!(i as f64 * 1.0f64, reader.read_double().unwrap());
                            }
                            MinorType::DECIMAL => {
                                entry_values += 1;
                                let d = reader.read_big_decimal().unwrap();
                                let (unscaled, _) = d.as_bigint_and_exponent();
                                assert_eq!(num_bigint::BigInt::from(i as i64 * 100), unscaled);
                            }
                            MinorType::VARBINARY => {
                                entry_values += 1;
                            }
                            MinorType::BIT => {
                                entry_values += 1;
                                assert_eq!(i % 2 == 1, reader.read_boolean().unwrap());
                            }
                            _ => {}
                        }
                    }
                    if entry_values > 0 {
                        actual += 1;
                    }
                }
                assert_eq!(actual_block.get_row_count(), actual, "failed for {}", vector.get_field().name());
            }
            _ => {}
        }
    }

    BlockUtils::unset_row(0, &mut actual_block).unwrap();

    for next in actual_block.get_fields() {
        let vector = actual_block.get_field_reader(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::DATEDAY
            | MinorType::DATEMILLI
            | MinorType::TINYINT
            | MinorType::UINT1
            | MinorType::SMALLINT
            | MinorType::UINT2
            | MinorType::UINT4
            | MinorType::INT
            | MinorType::UINT8
            | MinorType::BIGINT
            | MinorType::FLOAT4
            | MinorType::FLOAT8
            | MinorType::DECIMAL
            | MinorType::VARBINARY
            | MinorType::VARCHAR
            | MinorType::BIT
            | MinorType::STRUCT
            | MinorType::MAP => {
                vector.set_position(0);
                assert!(!vector.is_set(), "Failed for {} {}", vector.get_minor_type(), next.name());
            }
            MinorType::LIST => {}
            other => panic!("{} is not supported", other),
        }
    }
}

pub fn generate_test_schema() -> Schema {
    let mut schema_builder = SchemaBuilder::new();
    schema_builder.add_metadata("meta1", "meta-value-1");
    schema_builder.add_metadata("meta2", "meta-value-2");
    schema_builder.add_field_with_type("intfield1", DataType::Int32);
    schema_builder.add_field_with_type("doublefield2", DataType::Float64);
    schema_builder.add_field_with_type("varcharfield3", DataType::Utf8);
    schema_builder.add_field_with_type("datemillifield4", DataType::Date64);
    schema_builder.add_field_with_type("tinyintfield5", DataType::Int8);
    schema_builder.add_field_with_type("uint1field6", DataType::UInt8);
    schema_builder.add_field_with_type("smallintfield7", DataType::Int16);
    schema_builder.add_field_with_type("uint2field8", DataType::UInt16);
    schema_builder.add_field_with_type("datedayfield9", DataType::Date32);
    schema_builder.add_field_with_type("uint4field10", DataType::UInt32);
    schema_builder.add_field_with_type("bigintfield11", DataType::Int64);
    schema_builder.add_field_with_type("decimalfield12", decimal(10, 2));
    schema_builder.add_field_with_type("floatfield13", DataType::Float32);
    schema_builder.add_field_with_type("varbinaryfield14", DataType::Binary);
    schema_builder.add_field_with_type("bitfield15", DataType::Boolean);
    schema_builder.add_list_field("varcharlist16", DataType::Utf8);
    schema_builder.add_list_field("intlist17", DataType::Int32);
    schema_builder.add_list_field("bigintlist18", DataType::Int64);
    schema_builder.add_list_field("tinyintlist19", DataType::Int8);
    schema_builder.add_list_field("smallintlist20", DataType::Int16);
    schema_builder.add_list_field("float4list21", DataType::Float32);
    schema_builder.add_list_field("float8list22", DataType::Float64);
    schema_builder.add_list_field("shortdeclist23", decimal(10, 2));
    schema_builder.add_list_field("londdeclist24", decimal(21, 2));
    schema_builder.add_list_field("varbinarylist25", DataType::Binary);
    schema_builder.add_list_field("bitlist26", DataType::Boolean);
    schema_builder.add_struct_field("structField27");
    schema_builder.add_child_field("structField27", "nestedBigInt", DataType::Int64);
    schema_builder.add_child_field("structField27", "nestedString", DataType::Utf8);
    schema_builder.add_child_field("structField27", "tinyintcol", DataType::Int8);
    schema_builder.add_child_field("structField27", "smallintcol", DataType::Int16);
    schema_builder.add_child_field("structField27", "float4Col", DataType::Float32);
    schema_builder.add_child_field("structField27", "float8Col", DataType::Float64);
    schema_builder.add_child_field("structField27", "shortDecCol", decimal(10, 2));
    schema_builder.add_child_field("structField27", "longDecCol", decimal(21, 2));
    schema_builder.add_child_field("structField27", "binaryCol", DataType::Binary);
    schema_builder.add_child_field("structField27", "bitCol", DataType::Boolean);
    schema_builder.add_struct_field("structFieldNested28");
    schema_builder.add_child_field("structFieldNested28", "bitCol", DataType::Boolean);
    schema_builder.add_child(
        "structFieldNested28",
        FieldBuilder::new_builder("nestedStruct", DataType::Struct(Fields::empty()))
            .add_field_with_type("nestedString", DataType::Utf8, None)
            .add_field_with_type("nestedBigInt", DataType::Int64, None)
            .add_list_field("nestedList", DataType::Utf8)
            .add_list_field("nestedListDec", decimal(10, 2))
            .build(),
    );
    schema_builder.add_field(
        FieldBuilder::new_builder("simplemap", DataType::Map(Arc::new(Field::new(ENTRIES, DataType::Struct(Fields::empty()), false)), false))
            .add_field_with_nullable(
                ENTRIES,
                DataType::Struct(Fields::empty()),
                false,
                Some(vec![
                    Arc::new(FieldBuilder::new_builder_nullable(KEY, DataType::Utf8, false).build()),
                    Arc::new(FieldBuilder::new_builder(VALUE, DataType::Int32).build()),
                ]),
            )
            .build(),
    );
    schema_builder.build()
}

fn struct_resolver(seed: usize) -> impl Fn(&Field, &Value) -> Result<Value> {
    move |field: &Field, _value: &Value| -> Result<Value> {
        Ok(match field.name().as_str() {
            "nestedBigInt" => Value::Long(seed as i64),
            "nestedString" => Value::String((1000 + seed).to_string()),
            "tinyintcol" => Value::Byte(seed as i8),
            "smallintcol" => Value::Short(seed as i16),
            "nestedList" => Value::List(vec!["val1".into(), "val2".into()]),
            "nestedListDec" => Value::List(vec![Value::Double(2.0), Value::Double(2.2)]),
            "float4Col" => Value::Float(seed as f32 * 1.0),
            "float8Col" => Value::Double(seed as f64 * 2.0),
            "shortDecCol" => Value::Double(seed as f64 * 3.0),
            "longDecCol" => Value::Double(seed as f64 * 4.0),
            "binaryCol" => Value::Bytes(seed.to_string().into_bytes()),
            "bitCol" => Value::Boolean(seed % 2 == 1),
            "nestedStruct" => Value::Opaque(OpaqueValue::new(())),
            "key" => Value::String((1000 + seed).to_string()),
            "value" => Value::Int(seed as i32),
            other => return Err(FederationError::Runtime(format!("Unexpected field {}", other))),
        })
    }
}


pub fn generate_test_block(expected_allocator: &BlockAllocatorImpl, orig_schema: &Schema, expected_rows: usize) -> Block {
    let mut expected_block = expected_allocator.create_block(Arc::new(orig_schema.clone()));
    let mut field_count: usize = 1;
    for next in orig_schema.fields() {
        let vector = expected_block.get_field_vector_mut(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::DATEDAY => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count) as i32).unwrap();
                }
            }
            MinorType::UINT4 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 2) as i32).unwrap();
                }
            }
            MinorType::INT => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 3) as i32).unwrap();
                }
            }
            MinorType::FLOAT8 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, i as f64 * field_count as f64 * 1.1).unwrap();
                }
            }
            MinorType::VARCHAR => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count).to_string().into_bytes()).unwrap();
                }
            }
            MinorType::DATEMILLI => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 4) as i64).unwrap();
                }
            }
            MinorType::TINYINT => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 5) as i32).unwrap();
                }
            }
            MinorType::UINT1 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 6) as i32).unwrap();
                }
            }
            MinorType::SMALLINT => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 7) as i32).unwrap();
                }
            }
            MinorType::UINT2 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 8) as i32).unwrap();
                }
            }
            MinorType::UINT8 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 9) as i64).unwrap();
                }
            }
            MinorType::BIGINT => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 10) as i64).unwrap();
                }
            }
            MinorType::DECIMAL => {
                for i in 0..expected_rows {
                    let big_decimal = BigDecimal::from_f64((i * field_count) as f64 * 1.01).unwrap();
                    let big_decimal = big_decimal.with_scale_round(2, RoundingMode::HalfUp);
                    vector.set_safe(i, big_decimal).unwrap();
                }
            }
            MinorType::FLOAT4 => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count * 9) as i32).unwrap();
                }
            }
            MinorType::VARBINARY => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i * field_count).to_string().into_bytes()).unwrap();
                }
            }
            MinorType::BIT => {
                for i in 0..expected_rows {
                    vector.set_safe(i, (i % 2) as i32).unwrap();
                }
            }
            MinorType::STRUCT => {
                for i in 0..expected_rows {
                    let resolver = struct_resolver(i);
                    BlockUtils::set_complex_value(vector, i, &resolver, &Value::Opaque(OpaqueValue::new(()))).unwrap();
                }
            }
            MinorType::LIST => {
                let child = get_children(vector.get_field())[0].clone();
                for i in 0..expected_rows {
                    let values: Vec<Value> = match minor_type_of(child.data_type()) {
                        MinorType::BIGINT => vec![Value::Long(i as i64), Value::Long(i as i64 + 1), Value::Long(i as i64 + 2)],
                        MinorType::VARCHAR => vec![
                            Value::String((1000 + i).to_string()),
                            Value::String((1000 + i + 1).to_string()),
                            Value::String((1000 + i + 2).to_string()),
                        ],
                        MinorType::SMALLINT => vec![Value::Short((i + 1) as i16)],
                        MinorType::INT => vec![Value::Int(i as i32)],
                        MinorType::TINYINT => vec![Value::Byte(i as i8)],
                        MinorType::FLOAT4 => vec![Value::Float(i as f32 * 1.0)],
                        MinorType::FLOAT8 => vec![Value::Double(i as f64 * 1.0)],
                        MinorType::DECIMAL => vec![Value::Double(i as f64 * 1.0)],
                        MinorType::VARBINARY => vec![Value::Bytes(i.to_string().into_bytes())],
                        MinorType::BIT => vec![Value::Boolean(i % 2 == 1)],
                        _ => continue,
                    };
                    BlockUtils::set_complex_value(vector, i, &DEFAULT, &Value::List(values)).unwrap();
                }
            }
            MinorType::MAP => {
                for i in 0..expected_rows {
                    let resolver = struct_resolver(i);
                    BlockUtils::set_complex_value(vector, i, &resolver, &Value::Map(Vec::new())).unwrap();
                }
                let children: Vec<FieldRef> = get_children(vector.get_field());
                assert_eq!(1, children.len(), "Invalid Arrow Map schema");
                let key_value_struct_field = children[0].clone();
                assert!(
                    key_value_struct_field.name() == ENTRIES && matches!(key_value_struct_field.data_type(), DataType::Struct(_)),
                    "Invalid Arrow Map schema"
                );
                let key_value_children = get_children(&key_value_struct_field);
                assert_eq!(2, key_value_children.len(), "Invalid Arrow Map schema");
                assert!(
                    key_value_children[0].name() == KEY && key_value_children[1].name() == VALUE,
                    "Invalid Arrow Map schema"
                );
            }
            other => panic!("{} is not supported", other),
        }
        field_count += 1;
    }
    expected_block.set_row_count(expected_rows);
    expected_block
}

#[test]
fn list_of_lists_test() {
    let expected_allocator = BlockAllocatorImpl::new();
    let orig_schema = SchemaBuilder::new()
        .add_field(
            FieldBuilder::new_builder("outerlist", DataType::List(Arc::new(Field::new("", DataType::Null, true))))
                .add_list_field("innerList", DataType::Utf8)
                .build(),
        )
        .build();
    let mut expected_block = expected_allocator.create_block(Arc::new(orig_schema.clone()));
    let expected_rows = 200;
    for next in orig_schema.fields() {
        let vector = expected_block.get_field_vector_mut(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::LIST => {
                let child = get_children(vector.get_field())[0].clone();
                for i in 0..expected_rows {
                    let mut value = Vec::new();
                    match minor_type_of(child.data_type()) {
                        MinorType::LIST => {
                            value.push(Value::List(vec!["1000".into(), "1001".into(), "1002".into()]));
                        }
                        _ => panic!("{} is not supported", vector.get_minor_type()),
                    }
                    BlockUtils::set_complex_value(vector, i, &DEFAULT, &Value::List(value)).unwrap();
                }
            }
            other => panic!("{} is not supported", other),
        }
    }
    expected_block.set_row_count(expected_rows);

    let actual_block = round_trip(&expected_block, &orig_schema);
    for i in 0..actual_block.get_row_count() {
        BlockUtils::row_to_string(&actual_block, i).unwrap();
    }
    assert_eq!(expected_rows, actual_block.get_row_count(), "Row count missmatch");
    for next in actual_block.get_fields() {
        let vector = actual_block.get_field_reader(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::LIST => {
                let mut actual = 0;
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    let mut entry_values = 0;
                    while vector.next() {
                        let inner_reader = vector.reader().unwrap();
                        let mut j = 0;
                        while inner_reader.next() {
                            entry_values += 1;
                            assert_eq!((1000 + j).to_string(), inner_reader.reader().unwrap().read_text().unwrap());
                            j += 1;
                        }
                    }
                    if entry_values > 0 {
                        actual += 1;
                    }
                }
                assert_eq!(actual_block.get_row_count(), actual, "failed for {}", vector.get_field().name());
            }
            other => panic!("{} is not supported", other),
        }
    }
}

#[test]
fn list_of_structs_test() {
    let expected_allocator = BlockAllocatorImpl::new();
    let orig_schema = SchemaBuilder::new()
        .add_field(
            FieldBuilder::new_builder("outerlist", DataType::List(Arc::new(Field::new("", DataType::Null, true))))
                .add_field(
                    FieldBuilder::new_builder("innerStruct", DataType::Struct(Fields::empty()))
                        .add_string_field("varchar")
                        .add_big_int_field("bigint")
                        .build(),
                )
                .build(),
        )
        .build();
    let mut expected_block = expected_allocator.create_block(Arc::new(orig_schema.clone()));
    let expected_rows = 200;
    for next in orig_schema.fields() {
        let vector = expected_block.get_field_vector_mut(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::LIST => {
                let child = get_children(vector.get_field())[0].clone();
                for i in 0..expected_rows {
                    let mut value = Vec::new();
                    match minor_type_of(child.data_type()) {
                        MinorType::STRUCT => {
                            value.push(Value::struct_of([("varchar", Value::from("chars")), ("bigint", Value::Long(100))]));
                        }
                        _ => panic!("{} is not supported", vector.get_minor_type()),
                    }
                    BlockUtils::set_complex_value(vector, i, &DEFAULT, &Value::List(value)).unwrap();
                }
            }
            other => panic!("{} is not supported", other),
        }
    }
    expected_block.set_row_count(expected_rows);

    let actual_block = round_trip(&expected_block, &orig_schema);
    for i in 0..actual_block.get_row_count() {
        BlockUtils::row_to_string(&actual_block, i).unwrap();
    }
    assert_eq!(expected_rows, actual_block.get_row_count(), "Row count missmatch");
    for next in actual_block.get_fields() {
        let vector = actual_block.get_field_reader(next.name()).unwrap();
        match vector.get_minor_type() {
            MinorType::LIST => {
                let mut actual = 0;
                for i in 0..actual_block.get_row_count() {
                    vector.set_position(i);
                    let mut entry_values = 0;
                    while vector.next() {
                        entry_values += 1;
                        let inner = vector.reader().unwrap();
                        assert_eq!("chars", inner.reader_named("varchar").unwrap().read_text().unwrap());
                        assert_eq!(Some(100), inner.reader_named("bigint").unwrap().read_long());
                    }
                    if entry_values > 0 {
                        actual += 1;
                    }
                }
                assert_eq!(actual_block.get_row_count(), actual, "failed for {}", vector.get_field().name());
            }
            other => panic!("{} is not supported", other),
        }
    }
}

#[test]
fn struct_of_lists_test() {
    let expected_allocator = BlockAllocatorImpl::new();
    let orig_schema = SchemaBuilder::new()
        .add_field(
            FieldBuilder::new_builder("innerStruct", DataType::Struct(Fields::empty()))
                .add_string_field("varchar")
                .add_list_field("list", DataType::Utf8)
                .build(),
        )
        .build();
    let mut expected_block = expected_allocator.create_block(Arc::new(orig_schema.clone()));
    let expected_rows = 200;
    for next in orig_schema.fields() {
        let vector = expected_block.get_field_vector_mut(next.name()).unwrap();
        for i in 0..expected_rows {
            match vector.get_minor_type() {
                MinorType::STRUCT => {
                    let list = if i % 2 == 0 {
                        Value::List(vec![
                            Value::String(format!("value_0_{}", i)),
                            Value::String(format!("value_1_{}", i)),
                        ])
                    } else {
                        Value::Null
                    };
                    let value = Value::struct_of([("varchar", Value::from("chars")), ("list", list)]);
                    BlockUtils::set_complex_value(vector, i, &DEFAULT, &value).unwrap();
                }
                other => panic!("{} is not supported", other),
            }
        }
    }
    expected_block.set_row_count(expected_rows);

    let actual_block = round_trip(&expected_block, &orig_schema);
    for i in 0..actual_block.get_row_count() {
        BlockUtils::row_to_string(&actual_block, i).unwrap();
    }
    assert_eq!(expected_rows, actual_block.get_row_count(), "Row count missmatch");
    let mut actual_list_values = 0;
    let mut empty_list_values = 0;
    for next in actual_block.get_fields() {
        let vector = actual_block.get_field_reader(next.name()).unwrap();
        for i in 0..actual_block.get_row_count() {
            match vector.get_minor_type() {
                MinorType::STRUCT => {
                    vector.set_position(i);
                    assert_eq!("chars", vector.reader_named("varchar").unwrap().read_text().unwrap());
                    let list_reader = vector.reader_named("list").unwrap();
                    let mut found = 0;
                    while list_reader.next() {
                        assert_eq!(format!("value_{}_{}", found, i), list_reader.reader().unwrap().read_text().unwrap());
                        found += 1;
                        actual_list_values += 1;
                    }
                    if found == 0 {
                        empty_list_values += 1;
                    }
                }
                other => panic!("{} is not supported", other),
            }
        }
    }
    assert_eq!(200, actual_list_values);
    assert_eq!(100, empty_list_values);
}
