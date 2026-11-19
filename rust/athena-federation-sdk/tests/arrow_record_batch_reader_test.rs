use std::sync::Arc;

use arrow::datatypes::{DataType, Field, Schema};
use athena_federation_sdk::data::{BlockAllocatorImpl, RecordBatchSerDe};
use athena_federation_sdk::exceptions::JavaThrowable;
use athena_federation_sdk::FederationError;
use base64::Engine;

const NO_COLUMNS: &str = "/////1AAAAAUAAAAAAAAAAAACgAOAAYADQAIAAoAAAAAAAQAEAAAAAADCgAYAAwACAAEAAoAAAAUAAAAGAAAAAMAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";
const EMPTY_BUFFERS: &str = "/////3AAAAAQAAAAAAAKAA4ABgANAAgACgAAAAAABAAQAAAAAAMKAAwAAAAIAAQACgAAAAgAAAAsAAAAAgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAAAAAA";
const BUFFER_OUTSIDE_BODY: &str = "/////4AAAAAUAAAAAAAAAAAACgAOAAYADQAIAAoAAAAAAAQAEAAAAAADCgAYAAwACAAEAAoAAAAUAAAAOAAAAAMAAAAAAAAAAAAAAAIAAAAAAAAAAAAAAAgAAAAAAAAACAAAAAAAAAAQAAAAAAAAAAAAAAABAAAAAwAAAAAAAAAAAAAAAAAAAA==";

fn bytes(encoded: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap()
}

#[test]
fn reads_batch_without_columns_and_empty_body() {
    let allocator = BlockAllocatorImpl::new();
    let batch = RecordBatchSerDe::new(&allocator)
        .deserialize(&bytes(NO_COLUMNS))
        .unwrap();
    assert_eq!(3, batch.length());
    let decoded = batch.decode(Arc::new(Schema::empty())).unwrap();
    assert_eq!(3, decoded.num_rows());
    assert_eq!(0, decoded.num_columns());
}

#[test]
fn reads_batch_with_empty_buffers_and_empty_body() {
    let allocator = BlockAllocatorImpl::new();
    let batch = RecordBatchSerDe::new(&allocator)
        .deserialize(&bytes(EMPTY_BUFFERS))
        .unwrap();
    assert_eq!(0, batch.length());
    let schema = Schema::new(vec![Field::new("c", DataType::Int32, true)]);
    let decoded = batch.decode(Arc::new(schema)).unwrap();
    assert_eq!(0, decoded.num_rows());
}

#[test]
fn rejects_buffer_outside_empty_body() {
    let allocator = BlockAllocatorImpl::new();
    let error = RecordBatchSerDe::new(&allocator)
        .deserialize(&bytes(BUFFER_OUTSIDE_BODY))
        .unwrap_err();
    assert!(
        matches!(error, FederationError::IndexOutOfBounds(_)),
        "{error:?}"
    );
    assert_eq!(
        "index (8) must not be greater than size (0)",
        error.to_string()
    );
    assert_eq!(
        "java.lang.IndexOutOfBoundsException",
        error.java_class_name()
    );
}
