use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use arrow::datatypes::{DataType, TimeUnit};
use bigdecimal::BigDecimal;
use chrono::{NaiveDate, NaiveDateTime};
use num_bigint::BigInt;

use athena_federation_sdk::data::DefaultFieldResolver;
use athena_federation_sdk::data::*;
use athena_federation_sdk::data::{decimal, timestamp_tz};
use athena_federation_sdk::FederationError;

static PACKING_LOCK: Mutex<()> = Mutex::new(());

struct Setup {
    allocator: BlockAllocatorImpl,
    _guard: MutexGuard<'static, ()>,
}

impl Drop for Setup {
    fn drop(&mut self) {
        self.allocator.close();
        DateTimeFormatterUtil::enable_timezone_packing();
    }
}

fn setup() -> Setup {
    let guard = PACKING_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    DateTimeFormatterUtil::enable_timezone_packing();
    Setup {
        allocator: BlockAllocatorImpl::new(),
        _guard: guard,
    }
}

fn ldt_2020() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2020, 3, 18)
        .unwrap()
        .and_hms_opt(12, 54, 29)
        .unwrap()
}

fn set(block: &mut Block, name: &str, row: usize, value: impl Into<Value>) {
    BlockUtils::set_value(
        block.get_field_vector_mut(name).unwrap(),
        row,
        &value.into(),
    )
    .unwrap();
}

fn zoned(ldt: NaiveDateTime, zone: &str) -> Value {
    Value::ZonedDateTime(ZonedDateTime::of_local(ldt, ZoneId::of(zone).unwrap()))
}

fn assert_connector_error(result: athena_federation_sdk::Result<()>) {
    match result {
        Err(FederationError::AthenaConnector(_)) => {}
        other => panic!("expected AthenaConnectorException, got {other:?}"),
    }
}

#[test]
fn copy_rows() {
    let s = setup();
    for time_unit in [TimeUnit::Millisecond, TimeUnit::Microsecond] {
        if time_unit == TimeUnit::Microsecond {
            DateTimeFormatterUtil::disable_timezone_packing();
        }
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", decimal(38, 9))
            .add_field_with_type("col3", timestamp_tz(time_unit, "UTC"))
            .build();
        let ldt = ldt_2020();
        let date = Value::Date(ldt.and_utc().timestamp_millis());
        let mut src = s.allocator.create_block(Arc::new(schema.clone()));
        set(&mut src, "col1", 0, 10);
        set(&mut src, "col2", 0, BigDecimal::from(20));
        set(&mut src, "col3", 0, ldt);
        set(&mut src, "col1", 1, 11);
        set(&mut src, "col2", 1, BigDecimal::from(21));
        set(&mut src, "col3", 1, zoned(ldt, "-05:00"));
        set(&mut src, "col1", 2, 12);
        set(&mut src, "col2", 2, BigDecimal::from(22));
        set(&mut src, "col3", 2, date);
        src.set_row_count(3);

        let mut dst = s.allocator.create_block(Arc::new(schema));
        assert_eq!(3, BlockUtils::copy_rows(&src, &mut dst, 0, 2).unwrap());
        assert_eq!(src, dst);
    }
}

#[test]
fn is_null_row() {
    let s = setup();
    for time_unit in [TimeUnit::Millisecond, TimeUnit::Microsecond] {
        if time_unit == TimeUnit::Microsecond {
            DateTimeFormatterUtil::disable_timezone_packing();
        }
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", DataType::Int32)
            .add_field_with_type("col3", timestamp_tz(time_unit, "UTC"))
            .build();
        let ldt = chrono::Utc::now().naive_utc();
        let mut block = s.allocator.create_block(Arc::new(schema));
        set(&mut block, "col1", 0, 10);
        set(&mut block, "col2", 0, 20);
        set(&mut block, "col3", 0, ldt);
        set(&mut block, "col1", 1, 11);
        set(&mut block, "col2", 1, 21);
        set(&mut block, "col3", 1, zoned(ldt, "-05:00"));
        block.set_row_count(2);

        assert!(!BlockUtils::is_null_row(&block, 1).unwrap());
        BlockUtils::unset_row(1, &mut block).unwrap();
        assert!(BlockUtils::is_null_row(&block, 1).unwrap());
    }
}

#[test]
fn field_to_string() {
    let s = setup();
    for time_unit in [TimeUnit::Millisecond, TimeUnit::Microsecond] {
        let mut expected_string = "Block{rows=2, col1=[10, 11], col2=[2020-03-18T12:54:29Z[UTC], 2020-03-18T12:54:29-05:00], col3=[2020-03-18, 2019-12-29]}";
        if time_unit == TimeUnit::Microsecond {
            DateTimeFormatterUtil::disable_timezone_packing();
            expected_string = "Block{rows=2, col1=[10, 11], col2=[2020-03-18T12:54:29Z[UTC], 2020-03-18T17:54:29Z[UTC]], col3=[2020-03-18, 2019-12-29]}";
        }
        let schema = SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", timestamp_tz(time_unit, "UTC"))
            .add_field_with_type("col3", DataType::Date32)
            .build();
        let ldt = ldt_2020();
        let mut block = s.allocator.create_block(Arc::new(schema));
        set(&mut block, "col1", 0, 10);
        set(&mut block, "col2", 0, ldt);
        set(
            &mut block,
            "col3",
            0,
            Value::Date(ldt.and_utc().timestamp_millis()),
        );
        set(&mut block, "col1", 1, 11);
        set(&mut block, "col2", 1, zoned(ldt, "-05:00"));
        let parsed = NaiveDate::from_ymd_opt(2019, 12, 29)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        set(
            &mut block,
            "col3",
            1,
            Value::Date(parsed.and_utc().timestamp_millis()),
        );
        block.set_row_count(2);

        let actual = block.to_string();
        assert_eq!(expected_string, actual);
    }
}

#[test]
fn can_set_date() {
    let s = setup();
    let schema = SchemaBuilder::new_builder()
        .add_field_with_type("col1", DataType::Date32)
        .build();
    let date = NaiveDate::from_str("1998-01-01").unwrap();
    let mut block = s.allocator.create_block(Arc::new(schema));
    set(&mut block, "col1", 0, date);
}

#[test]
fn set_value_converts_other_numbers_exactly() {
    let s = setup();
    let schema = SchemaBuilder::new_builder()
        .add_field_with_type("int", DataType::Int32)
        .add_field_with_type("bigint", DataType::Int64)
        .add_field_with_type("smallint", DataType::Int16)
        .add_field_with_type("tinyint", DataType::Int8)
        .add_field_with_type("float8", DataType::Float64)
        .add_field_with_type("decimal", decimal(10, 2))
        .build();
    let mut block = s.allocator.create_block(Arc::new(schema));
    set(&mut block, "int", 0, BigDecimal::from_str("42").unwrap());
    set(
        &mut block,
        "bigint",
        0,
        BigDecimal::from_str("9223372036854775807").unwrap(),
    );
    set(
        &mut block,
        "smallint",
        0,
        BigDecimal::from_str("-300.00").unwrap(),
    );
    set(&mut block, "tinyint", 0, BigInt::from(-128));
    set(
        &mut block,
        "float8",
        0,
        BigDecimal::from_str("1.25").unwrap(),
    );
    set(&mut block, "decimal", 0, 7i64);

    assert_eq!(
        Value::Int(42),
        block.get_field_vector("int").unwrap().get_object(0)
    );
    assert_eq!(
        Value::Long(i64::MAX),
        block.get_field_vector("bigint").unwrap().get_object(0)
    );
    assert_eq!(
        Value::Short(-300),
        block.get_field_vector("smallint").unwrap().get_object(0)
    );
    assert_eq!(
        Value::Byte(-128),
        block.get_field_vector("tinyint").unwrap().get_object(0)
    );
    assert_eq!(
        Value::Double(1.25),
        block.get_field_vector("float8").unwrap().get_object(0)
    );
    assert_eq!(
        Value::Decimal(BigDecimal::from_str("7.00").unwrap()),
        block.get_field_vector("decimal").unwrap().get_object(0)
    );
}

#[test]
fn set_value_rejects_lossy_integer_conversions() {
    let s = setup();
    let schema = SchemaBuilder::new_builder()
        .add_field_with_type("int", DataType::Int32)
        .add_field_with_type("bigint", DataType::Int64)
        .add_field_with_type("tinyint", DataType::Int8)
        .build();
    let mut block = s.allocator.create_block(Arc::new(schema));
    let max_unsigned_long: BigInt = (BigInt::from(1) << 64) - 1;
    let dec = |s: &str| Value::Decimal(BigDecimal::from_str(s).unwrap());

    let int_vector = block.get_field_vector_mut("int").unwrap();
    assert_connector_error(BlockUtils::set_value(int_vector, 0, &dec("12.7")));
    assert_connector_error(BlockUtils::set_value(int_vector, 0, &dec("3000000000")));
    let bigint_vector = block.get_field_vector_mut("bigint").unwrap();
    assert_connector_error(BlockUtils::set_value(bigint_vector, 0, &dec("1.9")));
    assert_connector_error(BlockUtils::set_value(
        bigint_vector,
        0,
        &dec("9223372036854775808"),
    ));
    assert_connector_error(BlockUtils::set_value(
        bigint_vector,
        0,
        &Value::BigInteger(max_unsigned_long),
    ));
    assert_connector_error(BlockUtils::set_value(bigint_vector, 0, &Value::Double(1.9)));
    let tinyint_vector = block.get_field_vector_mut("tinyint").unwrap();
    assert_connector_error(BlockUtils::set_value(tinyint_vector, 0, &dec("300")));
}

#[test]
fn set_value_rounds_double_into_decimal_from_its_decimal_form() {
    let s = setup();
    let schema = SchemaBuilder::new_builder()
        .add_field_with_type("decimal", decimal(10, 2))
        .build();
    let mut block = s.allocator.create_block(Arc::new(schema));
    set(&mut block, "decimal", 0, 2.675f64);
    assert_eq!(
        Value::Decimal(BigDecimal::from_str("2.68").unwrap()),
        block.get_field_vector("decimal").unwrap().get_object(0)
    );
}

fn complex_write_error(field: arrow::datatypes::Field, value: &Value) -> String {
    let s = setup();
    let schema = arrow::datatypes::Schema::new(vec![field]);
    let mut block = s.allocator.create_block(Arc::new(schema));
    let vector = block.get_field_vector_mut("c").unwrap();
    BlockUtils::set_complex_value(vector, 0, &DefaultFieldResolver, value)
        .unwrap_err()
        .to_string()
}

fn nested(name: &str, data_type: DataType) -> arrow::datatypes::Field {
    arrow::datatypes::Field::new(name, data_type, true)
}

fn list_of(child: arrow::datatypes::Field) -> DataType {
    DataType::List(Arc::new(child))
}

fn struct_of(children: Vec<arrow::datatypes::Field>) -> DataType {
    DataType::Struct(children.into())
}

#[test]
fn write_errors_name_list_children_like_java_list_vectors() {
    let bad_int = || Value::String("x".to_string());
    let cases = [
        (
            nested("c", list_of(nested("", DataType::Int32))),
            Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(2)])]),
            "Unable to write value for field $data$ using value [1, 2] with minor type INT",
        ),
        (
            nested("c", list_of(nested("item", DataType::Int32))),
            Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(2)])]),
            "Unable to write value for field $data$ using value [1, 2] with minor type INT",
        ),
        (
            nested(
                "c",
                list_of(nested("inner", list_of(nested("leaf", DataType::Int32)))),
            ),
            Value::List(vec![Value::List(vec![bad_int()])]),
            "Unable to write value for field $data$ using value x with minor type INT",
        ),
        (
            nested("c", struct_of(vec![nested("a", DataType::Int32)])),
            Value::Map(vec![(Value::String("a".to_string()), bad_int())]),
            "Unable to write value for field a using value x with minor type INT",
        ),
        (
            nested(
                "c",
                list_of(nested(
                    "elem",
                    struct_of(vec![nested("a", DataType::Int32)]),
                )),
            ),
            Value::List(vec![Value::Map(vec![(
                Value::String("a".to_string()),
                bad_int(),
            )])]),
            "Unable to write value for field a using value x with minor type INT",
        ),
        (
            nested(
                "c",
                struct_of(vec![nested("l", list_of(nested("item", DataType::Int32)))]),
            ),
            Value::Map(vec![(
                Value::String("l".to_string()),
                Value::List(vec![bad_int()]),
            )]),
            "Unable to write value for field $data$ using value x with minor type INT",
        ),
    ];
    for (field, value, expected) in cases {
        let label = format!("{field:?}");
        assert_eq!(expected, complex_write_error(field, &value), "{label}");
    }
}
