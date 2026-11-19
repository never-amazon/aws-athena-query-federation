mod data_helpers;

use arrow::datatypes::{DataType, Field};
use athena_federation_sdk::data::block_utils;
use athena_federation_sdk::data::{minor_type_of, MinorType};
use athena_federation_sdk::data::{FieldResolver, FieldVector, Value};
use athena_federation_sdk::Result;
use proptest::prelude::*;
use std::sync::Arc;

use data_helpers::custom_field_vector::CustomFieldVector;
use data_helpers::fields_generator::{FieldsGenerator, MAP_KEY_NAME, MAP_VALUE_NAME};
use data_helpers::values_generator::ValuesGenerator;

pub struct ArrowToArrowResolver;

impl FieldResolver for ArrowToArrowResolver {
    fn get_field_value(&self, field: &Field, original_value: &Value) -> Result<Value> {
        if let DataType::Map(_, _) = field.data_type() {
            let input = match original_value {
                Value::Map(_) => original_value
                    .map_get(field.name())
                    .cloned()
                    .unwrap_or(Value::Null),
                other => other.clone(),
            };
            let items = match input {
                Value::Null => return Ok(Value::Null),
                Value::List(items) => items,
                other => {
                    return Err(athena_federation_sdk::FederationError::ClassCast(format!(
                        "class {} cannot be cast to JsonStringArrayList",
                        other.java_class_name()
                    )))
                }
            };
            let output: Vec<(Value, Value)> = items
                .iter()
                .map(|entry| {
                    (
                        entry.map_get(MAP_KEY_NAME).cloned().unwrap_or(Value::Null),
                        entry
                            .map_get(MAP_VALUE_NAME)
                            .cloned()
                            .unwrap_or(Value::Null),
                    )
                })
                .collect();
            return Ok(Value::Map(output));
        }
        if let Value::Map(_) = original_value {
            return Ok(original_value
                .map_get(field.name())
                .cloned()
                .unwrap_or(Value::Null));
        }
        Ok(original_value.clone())
    }

    fn get_map_key(&self, field: &Field, value: &Value) -> Result<Value> {
        self.get_field_value(field, value)
    }

    fn get_map_value(&self, field: &Field, value: &Value) -> Result<Value> {
        self.get_field_value(field, value)
    }
}

#[derive(Clone, Copy)]
enum Variant {
    ArrowToArrow,
    DefaultResolver,
}

fn get_value(
    variant: Variant,
    vector: &FieldVector,
    custom: &CustomFieldVector,
    pos: usize,
    resolver: &dyn FieldResolver,
) -> Result<Value> {
    match variant {
        Variant::DefaultResolver => Ok(custom.obj_list.get(pos).cloned().unwrap_or(Value::Null)),
        Variant::ArrowToArrow => {
            if vector.minor_type() == MinorType::MAP {
                resolver.get_field_value(vector.field(), &vector.get_object(pos))
            } else {
                Ok(vector.get_object(pos))
            }
        }
    }
}

fn vectors_equal(a: &FieldVector, b: &FieldVector) -> bool {
    a.field() == b.field()
        && a.value_count() == b.value_count()
        && (0..a.value_count()).all(|i| a.get_object(i) == b.get_object(i))
}

fn is_complex(field: &Field) -> bool {
    matches!(
        minor_type_of(field.data_type()),
        MinorType::LIST | MinorType::STRUCT | MinorType::MAP
    )
}

fn set_complex_values_sets_all_fields_correctly_given_any_input(
    variant: Variant,
    field: Field,
    seed: u64,
) -> bool {
    let field = Arc::new(field);
    let mut generator = ValuesGenerator::new(seed);
    let mut vector = FieldVector::new(field.clone());
    let mut custom = CustomFieldVector::new(field.clone());
    generator
        .generate_values_root(&mut vector, &mut custom)
        .expect("generate values");
    let value_count = vector.value_count();
    let mut output = FieldVector::new(field.clone());
    output.set_value_count(1);
    let resolver: Box<dyn FieldResolver> = match variant {
        Variant::ArrowToArrow => Box::new(ArrowToArrowResolver),
        Variant::DefaultResolver => Box::new(athena_federation_sdk::data::DefaultFieldResolver),
    };
    for i in 0..value_count {
        let value = match get_value(variant, &vector, &custom, i, resolver.as_ref()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("getValue failed: {e}");
                return false;
            }
        };
        let result = if is_complex(&field) {
            block_utils::set_complex_value(&mut output, i, resolver.as_ref(), &value)
        } else {
            block_utils::set_value(&mut output, i, &value)
        };
        if let Err(e) = result {
            eprintln!("DID NOT MATCH, write failed for {field:?}: {e}");
            return false;
        }
    }
    output.set_value_count(value_count);
    if vectors_equal(&vector, &output) {
        true
    } else {
        eprintln!("DID NOT MATCH\nField: {field:?}\nInput: {vector:?}\nOutput: {output:?}");
        false
    }
}

fn field_low_recursion() -> BoxedStrategy<Field> {
    FieldsGenerator::new(2).field()
}

fn field_high_recursion() -> BoxedStrategy<Field> {
    FieldsGenerator::new(5).field()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]
    #[test]
    fn set_complex_values_sets_all_fields_correctly_given_any_input_high_recursion(field in field_high_recursion(), seed in any::<u64>()) {
        prop_assert!(set_complex_values_sets_all_fields_correctly_given_any_input(Variant::ArrowToArrow, field, seed));
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]
    #[test]
    fn set_complex_values_sets_all_fields_correctly_given_any_input_all_combinations(field in field_low_recursion(), seed in any::<u64>()) {
        prop_assert!(set_complex_values_sets_all_fields_correctly_given_any_input(Variant::ArrowToArrow, field, seed));
    }
}

mod field_resolver_test {
    use super::*;

    fn field_low_recursion() -> BoxedStrategy<Field> {
        FieldsGenerator::with_complex_keys(2, false).field()
    }

    fn field_high_recursion() -> BoxedStrategy<Field> {
        FieldsGenerator::with_complex_keys(5, false).field()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]
        #[test]
        fn set_complex_values_sets_all_fields_correctly_given_any_input_high_recursion(field in field_high_recursion(), seed in any::<u64>()) {
            prop_assert!(set_complex_values_sets_all_fields_correctly_given_any_input(Variant::DefaultResolver, field, seed));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]
        #[test]
        fn set_complex_values_sets_all_fields_correctly_given_any_input_all_combinations(field in field_low_recursion(), seed in any::<u64>()) {
            prop_assert!(set_complex_values_sets_all_fields_correctly_given_any_input(Variant::DefaultResolver, field, seed));
        }
    }
}
