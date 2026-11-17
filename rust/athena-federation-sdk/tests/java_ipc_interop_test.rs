use std::sync::Arc;

use athena_federation_sdk::data::*;

const SCHEMA: &[u8] = include_bytes!("fixtures/java_arrow_18_3/schema.bin");
const BATCH: &[u8] = include_bytes!("fixtures/java_arrow_18_3/batch.bin");
const ROWS: &str = include_str!("fixtures/java_arrow_18_3/rows.txt");

fn rows(block: &Block) -> String {
    let mut dump = String::new();
    for i in 0..block.get_row_count() {
        dump.push_str(&BlockUtils::row_to_string(block, i).unwrap_or_else(|e| e.to_string()));
        dump.push('\n');
    }
    dump
}

fn load(allocator: &BlockAllocatorImpl, schema_bytes: &[u8], batch_bytes: &[u8]) -> Block {
    let schema = SchemaSerDe::new().deserialize(&mut &schema_bytes[..]).unwrap();
    let batch = RecordBatchSerDe::new(allocator).deserialize(batch_bytes).unwrap();
    let mut block = allocator.create_block(Arc::new(schema));
    block.load_arrow_record_batch(&batch).unwrap();
    block
}

#[test]
fn reads_java_arrow_ipc_messages() {
    let allocator = BlockAllocatorImpl::new();
    let block = load(&allocator, SCHEMA, BATCH);
    assert_eq!(23, block.get_row_count());
    assert_eq!(Some("v1"), block.get_meta_data("meta1"));
    assert_eq!(ROWS, rows(&block));
}

#[test]
fn rewrites_java_arrow_ipc_messages_without_loss() {
    let allocator = BlockAllocatorImpl::new();
    let block = load(&allocator, SCHEMA, BATCH);
    let mut schema_out = Vec::new();
    SchemaSerDe::new().serialize(block.get_schema(), &mut schema_out).unwrap();
    let mut batch_out = Vec::new();
    RecordBatchSerDe::new(&allocator)
        .serialize(&block.get_arrow_record_batch().unwrap(), &mut batch_out)
        .unwrap();
    let again = load(&allocator, &schema_out, &batch_out);
    assert_eq!(block.get_schema(), again.get_schema());
    assert_eq!(rows(&block), rows(&again));
    assert_eq!(block.get_record_batch().unwrap(), again.get_record_batch().unwrap());
}
