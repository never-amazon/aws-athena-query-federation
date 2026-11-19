use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Fields, Schema};
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, DefaultFieldResolver, FieldReader, Value,
};
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::handlers::{
    process_function, JavaType, UdfMethod, UdfMethods, UserDefinedFunctionHandler,
    UserDefinedFunctionStreamHandler,
};
use athena_federation_sdk::lambda::{LambdaError, RequestStreamHandler};
use athena_federation_sdk::metadata::ListSchemasRequest;
use athena_federation_sdk::request::{FederationRequest, FederationResponse, PingRequest};
use athena_federation_sdk::security::FederatedIdentity;
use athena_federation_sdk::serde::{ObjectMapper, SerdeVersion};
use athena_federation_sdk::udf::{UserDefinedFunctionRequest, UserDefinedFunctionType};
use athena_federation_sdk::{FederationError, Result};
use bigdecimal::{BigDecimal, RoundingMode};
use chrono::{Days, NaiveDate, NaiveDateTime};

const COLUMN_PREFIX: &str = "col_";
const HANDLER_CLASS: &str =
    "com.amazonaws.athena.connector.lambda.handlers.UserDefinedFunctionHandlerTest$TestUserDefinedFunctionHandler";

#[derive(Clone, Copy)]
enum ArgType {
    Byte,
    Short,
    Integer,
    Float,
    Double,
    String,
    ByteArray,
    Boolean,
    Long,
    BigDecimal,
    LocalDate,
    LocalDateTime,
    List,
    Map,
}

fn class_cast(value: &Value) -> FederationError {
    FederationError::ClassCast(format!("unexpected argument {value:?}"))
}

fn arg_i32(v: &Value) -> Result<i32> {
    match v {
        Value::Int(x) => Ok(*x),
        other => Err(class_cast(other)),
    }
}

fn arg_f64(v: &Value) -> Result<f64> {
    match v {
        Value::Double(x) => Ok(*x),
        other => Err(class_cast(other)),
    }
}

fn map_get<'a>(entries: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    entries
        .iter()
        .find(|(k, _)| matches!(k, Value::String(s) if s == key))
        .map(|(_, v)| v)
}

fn test_scalar_udf(col1: i32, col2: i32) -> i32 {
    col1 + col2
}

fn test_tiny_int(input: i8) -> i8 {
    input.wrapping_mul(2)
}

fn test_small_int(input: i16) -> i16 {
    input.wrapping_mul(3)
}

fn test_float4(col1: f32) -> f32 {
    col1 + 1.5
}

fn test_float8(input: f64) -> f64 {
    input * 2.5
}

fn test_bit(input: bool) -> bool {
    !input
}

fn test_big_int(input: i64) -> i64 {
    input * 2
}

fn test_varchar(input: &str) -> String {
    input.to_uppercase()
}

fn test_var_binary(input: &[u8]) -> Vec<u8> {
    input.iter().rev().copied().collect()
}

fn test_scalar_function_with_null_value(col1: Option<i32>) -> bool {
    col1.is_none()
}

fn test_list_type(input: &[i32]) -> Vec<i32> {
    input.iter().map(|v| v + 1).collect()
}

fn test_row_type(int_val: i32, double_val: f64) -> Vec<(Value, Value)> {
    vec![
        (Value::from("intVal"), Value::Int(int_val + 1)),
        (Value::from("doubleVal"), Value::Double(double_val + 1.0)),
    ]
}

fn test_decimal(input: &BigDecimal) -> BigDecimal {
    (input * BigDecimal::from(2)).with_scale_round(3, RoundingMode::HalfUp)
}

fn test_date_day(input: NaiveDate) -> NaiveDate {
    input
}

fn test_date_milli(input: NaiveDateTime) -> NaiveDateTime {
    input
}

fn udf_methods() -> UdfMethods {
    with_complex_methods(UdfMethods::new(HANDLER_CLASS))
        .with_method(UdfMethod::new(
            "test_zero_args",
            vec![],
            JavaType::Integer,
            |_| Ok(Value::Int(42)),
        ))
        .with_method(UdfMethod::new(
            "test_scalar_udf",
            vec![JavaType::Integer, JavaType::Integer],
            JavaType::Integer,
            |args| {
                Ok(Value::Int(test_scalar_udf(
                    arg_i32(&args[0])?,
                    arg_i32(&args[1])?,
                )))
            },
        ))
        .with_method(UdfMethod::new(
            "test_tiny_int",
            vec![JavaType::Byte],
            JavaType::Byte,
            |args| match &args[0] {
                Value::Byte(x) => Ok(Value::Byte(test_tiny_int(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_small_int",
            vec![JavaType::Short],
            JavaType::Short,
            |args| match &args[0] {
                Value::Short(x) => Ok(Value::Short(test_small_int(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_float4",
            vec![JavaType::Float],
            JavaType::Float,
            |args| match &args[0] {
                Value::Float(x) => Ok(Value::Float(test_float4(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_float8",
            vec![JavaType::Double],
            JavaType::Double,
            |args| Ok(Value::Double(test_float8(arg_f64(&args[0])?))),
        ))
        .with_method(UdfMethod::new(
            "test_bit",
            vec![JavaType::Boolean],
            JavaType::Boolean,
            |args| match &args[0] {
                Value::Boolean(x) => Ok(Value::Boolean(test_bit(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_big_int",
            vec![JavaType::Long],
            JavaType::Long,
            |args| match &args[0] {
                Value::Long(x) => Ok(Value::Long(test_big_int(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_varchar",
            vec![JavaType::String],
            JavaType::String,
            |args| match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::String(x) => Ok(Value::String(test_varchar(x))),
                other => Err(class_cast(other)),
            },
        ))
}

fn with_complex_methods(methods: UdfMethods) -> UdfMethods {
    methods
        .with_method(UdfMethod::new(
            "test_var_binary",
            vec![JavaType::ByteArray],
            JavaType::ByteArray,
            |args| match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Bytes(x) => Ok(Value::Bytes(test_var_binary(x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_scalar_function_with_null_value",
            vec![JavaType::Integer],
            JavaType::Boolean,
            |args| {
                let col1 = match &args[0] {
                    Value::Null => None,
                    other => Some(arg_i32(other)?),
                };
                Ok(Value::Boolean(test_scalar_function_with_null_value(col1)))
            },
        ))
        .with_method(UdfMethod::new(
            "test_list_type",
            vec![JavaType::List],
            JavaType::List,
            |args| match &args[0] {
                Value::List(items) => {
                    let ints = items.iter().map(arg_i32).collect::<Result<Vec<i32>>>()?;
                    Ok(Value::List(
                        test_list_type(&ints).into_iter().map(Value::Int).collect(),
                    ))
                }
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_row_type",
            vec![JavaType::Map],
            JavaType::Map,
            |args| match &args[0] {
                Value::Map(entries) => {
                    let int_val = arg_i32(map_get(entries, "intVal").unwrap_or(&Value::Null))?;
                    let double_val =
                        arg_f64(map_get(entries, "doubleVal").unwrap_or(&Value::Null))?;
                    Ok(Value::Map(test_row_type(int_val, double_val)))
                }
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_decimal",
            vec![JavaType::BigDecimal],
            JavaType::BigDecimal,
            |args| match &args[0] {
                Value::Null => Ok(Value::Null),
                Value::Decimal(x) => Ok(Value::Decimal(test_decimal(x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "decbad",
            vec![JavaType::BigDecimal],
            JavaType::BigDecimal,
            |args| match &args[0] {
                Value::Decimal(x) if x.sign() == bigdecimal::num_bigint::Sign::Plus => {
                    Ok(Value::Decimal(BigDecimal::from_str("1E40").unwrap()))
                }
                _ => Err(FederationError::runtime("after")),
            },
        ))
        .with_method(UdfMethod::new(
            "test_date_day",
            vec![JavaType::LocalDate],
            JavaType::LocalDate,
            |args| match &args[0] {
                Value::LocalDate(x) => Ok(Value::LocalDate(test_date_day(*x))),
                other => Err(class_cast(other)),
            },
        ))
        .with_method(UdfMethod::new(
            "test_date_milli",
            vec![JavaType::LocalDateTime],
            JavaType::LocalDateTime,
            |args| match &args[0] {
                Value::LocalDateTime(x) => Ok(Value::LocalDateTime(test_date_milli(*x))),
                other => Err(class_cast(other)),
            },
        ))
}

struct TestUserDefinedFunctionHandler {
    methods: UdfMethods,
}

impl UserDefinedFunctionHandler for TestUserDefinedFunctionHandler {
    fn source_type(&self) -> &'static str {
        "test_type"
    }

    fn udf_methods(&self) -> &UdfMethods {
        &self.methods
    }
}

struct Setup {
    handler: Arc<TestUserDefinedFunctionHandler>,
    allocator: Arc<dyn BlockAllocator>,
}

fn set_up() -> Setup {
    Setup {
        handler: Arc::new(TestUserDefinedFunctionHandler {
            methods: udf_methods(),
        }),
        allocator: Arc::new(BlockAllocatorImpl::new()),
    }
}

fn get_arrow_field(arg_type: ArgType, column_name: &str) -> Field {
    let data_type = match arg_type {
        ArgType::Byte => DataType::Int8,
        ArgType::Short => DataType::Int16,
        ArgType::Integer => DataType::Int32,
        ArgType::Float => DataType::Float32,
        ArgType::Double => DataType::Float64,
        ArgType::String => DataType::Utf8,
        ArgType::ByteArray => DataType::Binary,
        ArgType::Boolean => DataType::Boolean,
        ArgType::Long => DataType::Int64,
        ArgType::BigDecimal => DataType::Decimal128(38, 3),
        ArgType::LocalDate => DataType::Date32,
        ArgType::LocalDateTime => DataType::Date64,
        ArgType::List => DataType::List(Arc::new(Field::new(column_name, DataType::Int32, true))),
        ArgType::Map => DataType::Struct(Fields::from(vec![
            Field::new("intVal", DataType::Int32, true),
            Field::new("doubleVal", DataType::Float64, true),
        ])),
    };
    Field::new(column_name, data_type, true)
}

fn build_schema(types: &[ArgType]) -> Schema {
    let fields: Vec<Field> = types
        .iter()
        .enumerate()
        .map(|(i, t)| get_arrow_field(*t, &format!("{COLUMN_PREFIX}{i}")))
        .collect();
    Schema::new(fields)
}

fn write_column(block: &mut Block, name: &str, arg_type: ArgType, idx: usize) {
    let i = i32::try_from(idx).unwrap();
    let value: Value = match arg_type {
        ArgType::Byte => Value::Byte(i8::try_from(idx + 1).unwrap()),
        ArgType::Short => Value::Short(i16::try_from(idx + 1).unwrap()),
        ArgType::Integer => Value::Int(i + 100),
        ArgType::Float => Value::Float(f32::from(i16::try_from(idx).unwrap()) + 100.1),
        ArgType::Double => Value::Double(f64::from(i) + 0.5),
        ArgType::Boolean => Value::Boolean(idx.is_multiple_of(2)),
        ArgType::Long => Value::Long(i64::from(i) + 1),
        ArgType::String => Value::String(format!("row-{idx}")),
        ArgType::ByteArray => Value::Bytes(format!("Row-{}", idx + 1).into_bytes()),
        ArgType::BigDecimal => {
            Value::Decimal(BigDecimal::from(i) + BigDecimal::from_str("1.000").unwrap())
        }
        ArgType::LocalDate => Value::LocalDate(
            day_2020_01_01()
                .checked_add_days(Days::new(idx as u64))
                .unwrap(),
        ),
        ArgType::LocalDateTime => Value::LocalDateTime(
            day_2020_01_01()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .checked_add_days(Days::new(idx as u64))
                .unwrap(),
        ),
        ArgType::List => {
            block
                .set_complex_value(
                    name,
                    idx,
                    &DefaultFieldResolver,
                    vec![
                        Value::Int(i + 100),
                        Value::Int(i + 200),
                        Value::Int(i + 300),
                    ],
                )
                .unwrap();
            return;
        }
        ArgType::Map => {
            let input = Value::Map(vec![
                (Value::from("intVal"), Value::Int(i + 100)),
                (
                    Value::from("doubleVal"),
                    Value::Double(f64::from(i) + 200.2),
                ),
            ]);
            block
                .set_complex_value(name, idx, &DefaultFieldResolver, input)
                .unwrap();
            return;
        }
    };
    block.set_value(name, idx, value).unwrap();
}

fn day_2020_01_01() -> NaiveDate {
    NaiveDate::from_ymd_opt(2020, 1, 1).unwrap()
}

fn create_udf_request(
    s: &Setup,
    row_count: usize,
    return_type: ArgType,
    method_name: &str,
    non_null_data: bool,
    argument_types: &[ArgType],
) -> UserDefinedFunctionRequest {
    let input_schema = build_schema(argument_types);
    let output_schema = build_schema(&[return_type]);
    let mut block = s.allocator.create_block(Arc::new(input_schema));
    block.set_row_count(row_count);
    if non_null_data {
        for (i, arg_type) in argument_types.iter().enumerate() {
            let name = format!("{COLUMN_PREFIX}{i}");
            for idx in 0..row_count {
                write_column(&mut block, &name, *arg_type, idx);
            }
        }
    }
    UserDefinedFunctionRequest::new(
        None,
        block,
        output_schema,
        method_name,
        UserDefinedFunctionType::Scalar,
    )
}

fn process(
    s: &Setup,
    row_count: usize,
    return_type: ArgType,
    method_name: &str,
    argument_types: &[ArgType],
) -> Block {
    let request = create_udf_request(s, row_count, return_type, method_name, true, argument_types);
    let response = process_function(s.handler.as_ref(), s.allocator.as_ref(), &request).unwrap();
    let block = response.records().clone();
    assert_eq!(1, block.field_readers().len());
    assert_eq!(row_count, block.row_count());
    block
}

fn values_of(block: &Block) -> Vec<Value> {
    let readers = block.field_readers();
    let field_reader: &FieldReader<'_> = &readers[0];
    (0..block.row_count())
        .map(|pos| {
            let field_reader = field_reader.at(pos);
            field_reader.read_object()
        })
        .collect()
}

fn row(pos: usize) -> i32 {
    i32::try_from(pos).unwrap()
}

#[test]
fn test_invocation_with_basic_type() {
    let s = set_up();
    let block = process(
        &s,
        20,
        ArgType::Integer,
        "test_scalar_udf",
        &[ArgType::Integer, ArgType::Integer],
    );
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_scalar_udf(row(pos) + 100, row(pos) + 100);
        assert_eq!(Value::Int(expected), val);
    }
}

#[test]
fn invocation_with_tiny_int_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Byte, "test_tiny_int", &[ArgType::Byte]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_tiny_int(i8::try_from(pos + 1).unwrap());
        assert_eq!(Value::Byte(expected), val);
    }
}

#[test]
fn invocation_with_small_int_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Short, "test_small_int", &[ArgType::Short]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_small_int(i16::try_from(pos + 1).unwrap());
        assert_eq!(Value::Short(expected), val);
    }
}

#[test]
fn invocation_with_float4_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Float, "test_float4", &[ArgType::Float]);
    let readers = block.field_readers();
    for pos in 0..20 {
        let val = readers[0].at(pos).read_float().unwrap();
        let expected = test_float4(f32::from(i16::try_from(pos).unwrap()) + 100.1);
        assert!((expected - val).abs() < 0.0001, "{expected} {val}");
    }
}

#[test]
fn invocation_with_float8_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Double, "test_float8", &[ArgType::Double]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let Value::Double(val) = val else {
            panic!("expected a double, got {val:?}");
        };
        let expected = test_float8(f64::from(row(pos)) + 0.5);
        assert!((expected - val).abs() < 0.0001);
    }
}

#[test]
fn invocation_with_bit_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Boolean, "test_bit", &[ArgType::Boolean]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let val = matches!(val, Value::Boolean(true));
        assert_eq!(test_bit(pos % 2 == 0), val);
    }
}

#[test]
fn invocation_with_big_int_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Long, "test_big_int", &[ArgType::Long]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_big_int(i64::from(row(pos)) + 1);
        assert_eq!(Value::Long(expected), val);
    }
}

#[test]
fn invocation_with_varchar_type() {
    let s = set_up();
    let block = process(&s, 10, ArgType::String, "test_varchar", &[ArgType::String]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_varchar(&format!("row-{pos}"));
        assert_eq!(Value::String(expected), val);
    }
}

#[test]
fn invocation_with_var_binary_type() {
    let s = set_up();
    let block = process(
        &s,
        20,
        ArgType::ByteArray,
        "test_var_binary",
        &[ArgType::ByteArray],
    );
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_var_binary(format!("Row-{}", pos + 1).as_bytes());
        assert_eq!(Value::Bytes(expected), val);
    }
}

#[test]
fn test_invocation_with_list_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::List, "test_list_type", &[ArgType::List]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let p = row(pos);
        let expected: Vec<Value> = test_list_type(&[p + 100, p + 200, p + 300])
            .into_iter()
            .map(Value::Int)
            .collect();
        assert_eq!(Value::List(expected), val);
    }
}

#[test]
fn test_invocation_with_struct_type() {
    let s = set_up();
    let block = process(&s, 20, ArgType::Map, "test_row_type", &[ArgType::Map]);
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let Value::Map(actual) = val else {
            panic!("expected a struct value, got {val:?}");
        };
        let expected = test_row_type(row(pos) + 100, f64::from(row(pos)) + 200.2);
        for (key, value) in &expected {
            let Value::String(key) = key else {
                unreachable!("keys are strings")
            };
            assert_eq!(Some(value), map_get(&actual, key), "{key}");
        }
    }
}

#[test]
fn invocation_with_decimal_type() {
    let s = set_up();
    let block = process(
        &s,
        20,
        ArgType::BigDecimal,
        "test_decimal",
        &[ArgType::BigDecimal],
    );
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let input = BigDecimal::from(row(pos) + 1).with_scale(3);
        let expected = test_decimal(&input);
        let Value::Decimal(actual) = val else {
            panic!("expected a decimal, got {val:?}");
        };
        assert_eq!(expected, actual, "0");
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

#[test]
fn first_row_write_failure_wins_over_later_invoke_failure() {
    let s = set_up();
    let schema = build_schema(&[ArgType::BigDecimal]);
    let mut block = s.allocator.create_block(Arc::new(schema.clone()));
    block.set_row_count(2);
    for (row, text) in ["1.000", "-1.000"].into_iter().enumerate() {
        block
            .set_value(
                "col_0",
                row,
                Value::Decimal(BigDecimal::from_str(text).unwrap()),
            )
            .unwrap();
    }
    let request = UserDefinedFunctionRequest::new(
        None,
        block,
        schema,
        "decbad",
        UserDefinedFunctionType::Scalar,
    );
    let err = process_function(s.handler.as_ref(), s.allocator.as_ref(), &request).unwrap_err();
    assert_eq!(
        "java.lang.UnsupportedOperationException",
        err.java_class_name()
    );
    assert_eq!(
        "BigDecimal precision cannot be greater than that in the Arrow vector: 44 > 38",
        err.to_string()
    );
}

#[test]
fn invocation_with_date_day_type() {
    let s = set_up();
    let block = process(
        &s,
        20,
        ArgType::LocalDate,
        "test_date_day",
        &[ArgType::LocalDate],
    );
    let base = NaiveDate::from_ymd_opt(2019, 12, 31).unwrap();
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_date_day(base.checked_add_days(Days::new(pos as u64 + 1)).unwrap());
        let Value::Int(epoch_day) = val else {
            panic!("expected epoch days, got {val:?}");
        };
        let actual = NaiveDate::from_epoch_days(epoch_day).unwrap();
        assert_eq!(expected, actual, "{pos}");
    }
}

#[test]
fn invocation_with_date_milli_type() {
    let s = set_up();
    let block = process(
        &s,
        20,
        ArgType::LocalDateTime,
        "test_date_milli",
        &[ArgType::LocalDateTime],
    );
    let base = NaiveDateTime::from_str("2019-12-31T00:00:00").unwrap();
    for (pos, val) in values_of(&block).into_iter().enumerate() {
        let expected = test_date_milli(base.checked_add_days(Days::new(pos as u64 + 1)).unwrap());
        assert_eq!(Value::LocalDateTime(expected), val, "{pos}");
    }
}

#[test]
fn test_invocation_with_null_value() {
    let s = set_up();
    let request = create_udf_request(
        &s,
        20,
        ArgType::Boolean,
        "test_scalar_function_with_null_value",
        false,
        &[ArgType::Integer],
    );
    let response = process_function(s.handler.as_ref(), s.allocator.as_ref(), &request).unwrap();
    let block = response.records();
    assert_eq!(1, block.field_readers().len());
    assert_eq!(20, block.row_count());
    let readers = block.field_readers();
    for pos in 0..20 {
        let reader = readers[0].at(pos);
        assert!(reader.is_set());
        let expected = test_scalar_function_with_null_value(None);
        assert_eq!(Some(expected), reader.read_boolean());
    }
}

#[tokio::test]
async fn test_request_type_validation() {
    let s = set_up();
    let federation_request: FederationRequest =
        ListSchemasRequest::new(None, Some("dummy_catalog".into()), "dummy_qid").into();
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&s.allocator));
    let input_data = object_mapper
        .write_request(&federation_request)
        .map(String::into_bytes)
        .unwrap();
    let err = UserDefinedFunctionStreamHandler::new(s.handler)
        .handle_request(&input_data, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Expected a UserDefinedFunctionRequest but found"),
        "{err}"
    );
}

#[tokio::test]
async fn ping_handle_request() {
    let s = set_up();
    let identity =
        FederatedIdentity::new("arn", "account", HashMap::new(), Vec::new(), HashMap::new());
    let ping_request: FederationRequest =
        PingRequest::new(Some(identity), "catalog", "queryId").into();
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&s.allocator));
    let input = object_mapper
        .write_request(&ping_request)
        .map(String::into_bytes)
        .unwrap();
    let output = UserDefinedFunctionStreamHandler::new(Arc::clone(&s.handler))
        .handle_request(&input, None)
        .await
        .unwrap();
    let response = object_mapper.read_response(&output).unwrap();
    assert!(matches!(response, FederationResponse::Ping(_)));
}

#[tokio::test]
async fn udf_handle_request() {
    let s = set_up();
    let udf_request =
        create_udf_request(&s, 5, ArgType::Long, "test_big_int", true, &[ArgType::Long]);
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&s.allocator));
    let request: FederationRequest = udf_request.into();
    let input = object_mapper
        .write_request(&request)
        .map(String::into_bytes)
        .unwrap();
    let output = UserDefinedFunctionStreamHandler::new(Arc::clone(&s.handler))
        .handle_request(&input, None)
        .await
        .unwrap();
    let base_response = object_mapper.read_response(&output).unwrap();
    assert!(matches!(
        base_response,
        FederationResponse::UserDefinedFunction(_)
    ));
}

#[tokio::test]
async fn zero_argument_udf_handle_request() {
    let s = set_up();
    let row_count = 3;
    let udf_request =
        create_udf_request(&s, row_count, ArgType::Integer, "test_zero_args", true, &[]);
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&s.allocator));
    let request: FederationRequest = udf_request.into();
    let input = object_mapper
        .write_request(&request)
        .map(String::into_bytes)
        .unwrap();
    let output = UserDefinedFunctionStreamHandler::new(Arc::clone(&s.handler))
        .handle_request(&input, None)
        .await
        .unwrap();
    let FederationResponse::UserDefinedFunction(response) =
        object_mapper.read_response(&output).unwrap()
    else {
        panic!("expected a UserDefinedFunctionResponse");
    };
    let block = response.records();
    assert_eq!(row_count, block.row_count());
    assert_eq!(vec![Value::Int(42); row_count], values_of(block));
}

#[test]
fn test_method_not_found() {
    let s = set_up();
    let request = create_udf_request(
        &s,
        20,
        ArgType::Integer,
        "method_that_does_not_exsit",
        true,
        &[ArgType::Integer, ArgType::Integer],
    );
    let err = process_function(s.handler.as_ref(), s.allocator.as_ref(), &request).unwrap_err();
    assert!(
        matches!(err, FederationError::AthenaConnector(_)),
        "{err:?}"
    );
}

#[tokio::test]
async fn udf_handler_runs_as_request_stream_handler() {
    let Setup { handler, allocator } = set_up();
    let stream_handler = UserDefinedFunctionStreamHandler::new(handler);
    let object_mapper = ObjectMapper::new(SerdeVersion::CURRENT, Arc::clone(&allocator));
    let wrong: FederationRequest =
        ListSchemasRequest::new(None, Some("dummy_catalog".into()), "dummy_qid").into();
    let input = object_mapper
        .write_request(&wrong)
        .map(String::into_bytes)
        .unwrap();
    let err = RequestStreamHandler::handle_request(&stream_handler, &input, None)
        .await
        .unwrap_err();
    assert_eq!(
        "errorType: com.amazonaws.athena.connector.lambda.exceptions.AthenaConnectorException\n\
         errorMessage: Expected a UserDefinedFunctionRequest but found class com.amazonaws.athena.connector.lambda.metadata.ListSchemasRequest\n\
         errorCode: InvalidInputException\n",
        LambdaError::new(&err).to_report()
    );
    assert_runnable(&stream_handler);
}

fn assert_runnable<H: RequestStreamHandler + 'static>(_handler: &H) {}
