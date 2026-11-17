use std::sync::Arc;

use super::block::Block;
use super::block_writer::{BlockWriter, RowWriter};
use crate::domain::predicate::ConstraintEvaluator;
use crate::exceptions::Result;

pub struct SimpleBlockWriter<'a> {
    block: &'a mut Block,
}

impl<'a> SimpleBlockWriter<'a> {
    pub fn new(block: &'a mut Block) -> SimpleBlockWriter<'a> {
        SimpleBlockWriter { block }
    }

    pub fn block(&self) -> &Block {
        self.block
    }
}

impl BlockWriter for SimpleBlockWriter<'_> {
    fn write_rows(&mut self, row_writer: &mut dyn RowWriter) -> Result<()> {
        let row_count = self.block.row_count();
        let rows = row_writer.write_rows(self.block, row_count)?;
        if rows > 0 {
            self.block.set_row_count(row_count + rows);
        }
        Ok(())
    }

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        self.block.constraint_evaluator().clone()
    }
}
