use std::fmt::Write as _;
use std::str::FromStr;
use std::sync::Arc;

use arrow::array::{
    Array, BinaryArray, BooleanArray, Date32Array, Decimal128Array, Float64Array, Int32Array,
    Int64Array, StringArray,
};
use arrow::datatypes::{DataType, Schema};
use athena_federation_sdk::data::{
    Block, BlockAllocator, BlockAllocatorImpl, BlockUtils, SchemaBuilder, Value,
};
use athena_federation_sdk::security::{
    AesGcmBlockCrypto, BlockCrypto, EncryptionKey, NoOpBlockCrypto,
};
use bigdecimal::BigDecimal;
use chrono::NaiveDate;

const PLAIN: &[u8] = include_bytes!("fixtures/aes_gcm_java/plain.bin");
const AES256: &[u8] = include_bytes!("fixtures/aes_gcm_java/aes256.bin");
const AES128: &[u8] = include_bytes!("fixtures/aes_gcm_java/aes128.bin");
const RAW256: &[u8] = include_bytes!("fixtures/aes_gcm_java/raw256.bin");
const ROWS: &str = include_str!("fixtures/aes_gcm_java/rows.txt");
const RUST_PLAIN: &[u8] = include_bytes!("fixtures/aes_gcm_java/rust_plain.bin");
const RUST_AES256: &[u8] = include_bytes!("fixtures/aes_gcm_java/rust_aes256.bin");
const RUST_AES128: &[u8] = include_bytes!("fixtures/aes_gcm_java/rust_aes128.bin");

fn seq(n: usize, start: u8) -> Vec<u8> {
    (0..n)
        .map(|i| start.wrapping_add(u8::try_from(i).unwrap()))
        .collect()
}

fn key256() -> EncryptionKey {
    EncryptionKey::new(seq(32, 0), seq(12, 100))
}

fn key128() -> EncryptionKey {
    EncryptionKey::new(seq(16, 50), seq(12, 200))
}

fn schema() -> Arc<Schema> {
    Arc::new(
        SchemaBuilder::new_builder()
            .add_field_with_type("col1", DataType::Int32)
            .add_field_with_type("col2", DataType::Utf8)
            .add_field_with_type("col3", DataType::Int64)
            .add_field_with_type("col4", DataType::Float64)
            .add_field_with_type("col5", DataType::Boolean)
            .add_field_with_type("col6", DataType::Date32)
            .add_field_with_type("col7", DataType::Decimal128(10, 2))
            .add_field_with_type("col8", DataType::Binary)
            .build(),
    )
}

fn set(block: &mut Block, field: &str, row: usize, value: &Value) {
    BlockUtils::set_value(block.get_field_vector_mut(field).unwrap(), row, value).unwrap();
}

fn date(y: i32, m: u32, d: u32) -> Value {
    Value::LocalDate(NaiveDate::from_ymd_opt(y, m, d).unwrap())
}

fn decimal(s: &str) -> Value {
    Value::Decimal(BigDecimal::from_str(s).unwrap())
}

fn rust_block(allocator: &BlockAllocatorImpl) -> Block {
    let mut block = allocator.create_block(schema());
    set(&mut block, "col1", 0, &Value::Int(100));
    set(&mut block, "col2", 0, &Value::String("VarChar".into()));
    set(&mut block, "col3", 0, &Value::Long(9_000_000_000));
    set(&mut block, "col4", 0, &Value::Double(1.5));
    set(&mut block, "col5", 0, &Value::Boolean(true));
    set(&mut block, "col6", 0, &date(2020, 3, 18));
    set(&mut block, "col7", 0, &decimal("12.34"));
    set(&mut block, "col8", 0, &Value::Bytes(vec![1, 2, 3]));
    set(&mut block, "col1", 1, &Value::Int(101));
    set(&mut block, "col2", 1, &Value::String("VarChar1".into()));
    set(&mut block, "col6", 1, &date(2000, 1, 1));
    set(&mut block, "col1", 2, &Value::Int(-7));
    set(&mut block, "col2", 2, &Value::String("été".into()));
    set(&mut block, "col3", 2, &Value::Long(-1));
    set(&mut block, "col4", 2, &Value::Double(-0.25));
    set(&mut block, "col5", 2, &Value::Boolean(false));
    set(&mut block, "col6", 2, &date(1969, 12, 31));
    set(&mut block, "col7", 2, &decimal("-0.01"));
    set(&mut block, "col8", 2, &Value::Bytes(Vec::new()));
    block.set_row_count(3);
    block
}

fn cell(array: &dyn Array, row: usize) -> String {
    if array.is_null(row) {
        return "null".to_string();
    }
    let any = array.as_any();
    if let Some(a) = any.downcast_ref::<Int32Array>() {
        return a.value(row).to_string();
    }
    if let Some(a) = any.downcast_ref::<StringArray>() {
        return a.value(row).to_string();
    }
    if let Some(a) = any.downcast_ref::<Int64Array>() {
        return a.value(row).to_string();
    }
    if let Some(a) = any.downcast_ref::<Float64Array>() {
        return format!("{:?}", a.value(row));
    }
    if let Some(a) = any.downcast_ref::<BooleanArray>() {
        return a.value(row).to_string();
    }
    if let Some(a) = any.downcast_ref::<Date32Array>() {
        return a.value(row).to_string();
    }
    if let Some(a) = any.downcast_ref::<Decimal128Array>() {
        return a.value_as_string(row);
    }
    if let Some(a) = any.downcast_ref::<BinaryArray>() {
        let parts: Vec<String> = a
            .value(row)
            .iter()
            .map(|b| b.cast_signed().to_string())
            .collect();
        return format!("[{}]", parts.join(", "));
    }
    panic!("unexpected array type {:?}", array.data_type());
}

fn rows(block: &Block) -> String {
    let batch = block.get_record_batch().unwrap();
    let mut dump = String::new();
    for row in 0..batch.num_rows() {
        for (i, field) in batch.schema().fields().iter().enumerate() {
            write!(
                dump,
                "{}={};",
                field.name(),
                cell(batch.column(i).as_ref(), row)
            )
            .unwrap();
        }
        dump.push('\n');
    }
    dump
}

#[test]
fn decrypts_java_aes256_block() {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let crypto = AesGcmBlockCrypto::new(allocator.clone());
    assert_eq!(PLAIN.len() + 16, AES256.len());
    let block = crypto
        .decrypt_with_schema(Some(&key256()), AES256, schema())
        .unwrap();
    assert_eq!(3, block.row_count());
    assert_eq!(ROWS, rows(&block));
    assert_eq!(rust_block(&allocator), block);
}

#[test]
fn decrypts_java_aes128_block() {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let crypto = AesGcmBlockCrypto::new(allocator.clone());
    let block = crypto
        .decrypt_with_schema(Some(&key128()), AES128, schema())
        .unwrap();
    assert_eq!(ROWS, rows(&block));
}

#[test]
fn decrypts_java_raw_bytes() {
    let crypto = AesGcmBlockCrypto::new(Arc::new(BlockAllocatorImpl::new()));
    assert_eq!(
        b"hello spill".to_vec(),
        crypto.decrypt(Some(&key256()), RAW256).unwrap()
    );
}

#[test]
fn reads_java_unencrypted_block() {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let block = NoOpBlockCrypto::new(allocator.clone())
        .decrypt_with_schema(None, PLAIN, schema())
        .unwrap();
    assert_eq!(ROWS, rows(&block));
}

#[test]
fn rust_raw_ciphertext_is_identical_to_java() {
    let crypto = AesGcmBlockCrypto::new(Arc::new(BlockAllocatorImpl::new()));
    assert_eq!(
        RAW256,
        crypto
            .encrypt_with_bytes(Some(&key256()), b"hello spill")
            .unwrap()
            .as_slice()
    );
}

#[test]
fn rust_block_ciphertext_matches_java_verified_fixture() {
    let allocator = Arc::new(BlockAllocatorImpl::new());
    let block = rust_block(&allocator);
    assert_eq!(ROWS, rows(&block));
    let crypto = AesGcmBlockCrypto::new(allocator.clone());
    let plain = NoOpBlockCrypto::new(allocator.clone())
        .encrypt(None, &block)
        .unwrap();
    let aes256 = crypto.encrypt(Some(&key256()), &block).unwrap();
    let aes128 = crypto.encrypt(Some(&key128()), &block).unwrap();
    assert_eq!(RUST_PLAIN, plain.as_slice());
    assert_eq!(RUST_AES256, aes256.as_slice());
    assert_eq!(RUST_AES128, aes128.as_slice());
    assert_eq!(plain.len() + 16, aes256.len());
    assert_eq!(
        crypto.encrypt_with_bytes(Some(&key256()), &plain).unwrap(),
        aes256
    );
    assert_ne!([0xffu8; 4], PLAIN[..4]);
    assert_ne!([0xffu8; 4], plain[..4]);
}

#[test]
fn wrong_key_fails_on_java_ciphertext() {
    let crypto = AesGcmBlockCrypto::new(Arc::new(BlockAllocatorImpl::new()));
    let ex = crypto
        .decrypt_with_schema(Some(&key128()), AES256, schema())
        .unwrap_err();
    assert!(ex.as_connector_exception().is_some());
}
