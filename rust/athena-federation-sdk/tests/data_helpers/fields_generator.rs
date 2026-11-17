use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, TimeUnit};
use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;

pub const MAP_DATA_VECTOR_NAME: &str = "entries";
pub const MAP_KEY_NAME: &str = "key";
pub const MAP_VALUE_NAME: &str = "value";

const DEFAULT_NULLABLE: bool = true;

#[derive(Debug, Clone, Copy)]
pub struct FieldsGenerator {
    allow_complex_keys: bool,
    max_recursion_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Complex {
    List,
    Struct,
    Map,
}

fn int_type() -> BoxedStrategy<DataType> {
    prop_oneof![
        Just(DataType::Int8),
        Just(DataType::Int16),
        Just(DataType::Int32),
        Just(DataType::Int64),
        Just(DataType::UInt8),
        Just(DataType::UInt16),
        Just(DataType::UInt32),
        Just(DataType::UInt64),
    ]
    .boxed()
}

fn date_type() -> BoxedStrategy<DataType> {
    prop_oneof![Just(DataType::Date32), Just(DataType::Date64)].boxed()
}

fn floating_point_type() -> BoxedStrategy<DataType> {
    prop_oneof![Just(DataType::Float32), Just(DataType::Float64)].boxed()
}

pub fn timestamp_type() -> BoxedStrategy<DataType> {
    let zones = prop::sample::select(vec!["UTC", "America/Chicago", "Europe/Paris", "Asia/Tokyo", "Australia/Sydney"]);
    let units = prop::sample::select(vec![
        TimeUnit::Second,
        TimeUnit::Millisecond,
        TimeUnit::Microsecond,
        TimeUnit::Nanosecond,
    ]);
    (zones, units)
        .prop_map(|(z, u)| DataType::Timestamp(u, Some(Arc::from(z))))
        .boxed()
}

fn decimal_type() -> BoxedStrategy<DataType> {
    (1u8..=20)
        .prop_flat_map(|precision| (Just(precision), 0i8..(precision as i8), 0usize..3))
        .prop_map(|(p, s, width)| match width {
            0 => DataType::Decimal32(p, s),
            1 => DataType::Decimal64(p, s),
            _ => DataType::Decimal128(p, s),
        })
        .boxed()
}

fn primitive_field_type() -> BoxedStrategy<DataType> {
    prop_oneof![
        Just(DataType::Binary),
        Just(DataType::Boolean),
        date_type(),
        decimal_type(),
        int_type(),
        floating_point_type(),
        Just(DataType::Utf8),
    ]
    .boxed()
}

impl FieldsGenerator {
    pub fn new(max_recursion_depth: usize) -> FieldsGenerator {
        FieldsGenerator {
            allow_complex_keys: true,
            max_recursion_depth,
        }
    }

    pub fn with_complex_keys(max_recursion_depth: usize, allow_complex_keys: bool) -> FieldsGenerator {
        FieldsGenerator {
            allow_complex_keys,
            max_recursion_depth,
        }
    }

    pub fn field(self) -> BoxedStrategy<Field> {
        self.field_at(0, None, DEFAULT_NULLABLE)
    }

    fn field_at(self, depth: usize, name: Option<&'static str>, nullable: bool) -> BoxedStrategy<Field> {
        let next = depth + 1;
        let name_strategy: BoxedStrategy<String> = match name {
            Some(n) => Just(n.to_string()).boxed(),
            None => ("[a-zA-Z][a-zA-Z0-9_]{0,5}", 0u32..1_000_000)
                .prop_map(|(s, n)| format!("{}{}", s, n))
                .boxed(),
        };
        let primitive = (name_strategy.clone(), primitive_field_type())
            .prop_map(move |(n, t)| Field::new(n, t, nullable))
            .boxed();
        if next >= self.max_recursion_depth {
            return primitive;
        }
        let complex = prop_oneof![Just(Complex::List), Just(Complex::Struct), Just(Complex::Map)]
            .prop_flat_map(move |kind| {
                let name_strategy = name_strategy.clone();
                match kind {
                    Complex::List => (name_strategy, self.field_at(next, None, DEFAULT_NULLABLE))
                        .prop_map(move |(n, child)| Field::new(n, DataType::List(Arc::new(child)), nullable))
                        .boxed(),
                    Complex::Struct => (
                        name_strategy,
                        prop::collection::vec(self.field_at(next, None, DEFAULT_NULLABLE), 1..=3),
                    )
                        .prop_map(move |(n, children)| {
                            let children: Vec<Field> = children
                                .into_iter()
                                .enumerate()
                                .map(|(i, c)| {
                                    let unique = format!("{}_{}", c.name(), i);
                                    c.with_name(unique)
                                })
                                .collect();
                            Field::new(n, DataType::Struct(Fields::from(children)), nullable)
                        })
                        .boxed(),
                    Complex::Map => {
                        let key = if self.allow_complex_keys {
                            self.field_at(next, Some(MAP_KEY_NAME), false)
                        } else {
                            self.field_at(self.max_recursion_depth, Some(MAP_KEY_NAME), false)
                        };
                        let value = self.field_at(next, Some(MAP_VALUE_NAME), DEFAULT_NULLABLE);
                        (name_strategy, key, value)
                            .prop_map(move |(n, k, v)| {
                                let entries = Field::new(
                                    MAP_DATA_VECTOR_NAME,
                                    DataType::Struct(Fields::from(vec![k, v])),
                                    false,
                                );
                                Field::new(n, DataType::Map(Arc::new(entries), true), nullable)
                            })
                            .boxed()
                    }
                }
            })
            .boxed();
        prop_oneof![primitive, complex].boxed()
    }
}
