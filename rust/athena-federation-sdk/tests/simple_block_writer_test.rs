use std::sync::Arc;

use athena_federation_sdk::data::{Block, SchemaBuilder, SimpleBlockWriter};
use athena_federation_sdk::domain::predicate::ConstraintEvaluator;
use athena_federation_sdk::FederationError;

fn new_block() -> Block {
    let schema = SchemaBuilder::new_builder().add_int_field("col1").build();
    Block::new("test-allocator", Arc::new(schema))
}

#[test]
fn test_write_rows() {
    let mut block = new_block();
    let mut calls: Vec<usize> = Vec::new();
    {
        let mut writer = SimpleBlockWriter::new(&mut block);
        let mut row_writer =
            |_: &mut Block, row_num: usize| -> athena_federation_sdk::Result<usize> {
                calls.push(row_num);
                Ok(5)
            };
        writer.write_rows(&mut row_writer).unwrap();
    }
    assert_eq!(vec![0usize], calls);
    assert_eq!(5, block.row_count());
}

#[test]
fn test_write_rows_with_exception() {
    let mut block = new_block();
    let mut writer = SimpleBlockWriter::new(&mut block);
    let mut row_writer = |_: &mut Block, _: usize| -> athena_federation_sdk::Result<usize> {
        Err(FederationError::runtime("Simulated exception"))
    };
    let err = writer.write_rows(&mut row_writer).unwrap_err();
    assert_eq!("Simulated exception", err.to_string());
}

#[test]
fn test_get_constraint_evaluator() {
    let mut block = new_block();
    let evaluator = Arc::new(ConstraintEvaluator::empty_evaluator());
    block.constrain(Some(evaluator.clone()));
    let writer = SimpleBlockWriter::new(&mut block);
    let result = writer.constraint_evaluator();
    assert!(Arc::ptr_eq(&evaluator, &result));
}
