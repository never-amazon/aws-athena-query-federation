use std::sync::Arc;

use super::block::Block;
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

    pub fn write_rows(
        &mut self,
        row_writer: &mut dyn FnMut(&mut Block, usize) -> Result<usize>,
    ) -> Result<()> {
        let row_count = self.block.row_count();
        let rows = row_writer(self.block, row_count)?;
        if rows > 0 {
            self.block.set_row_count(row_count + rows);
        }
        Ok(())
    }

    pub fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        self.block.constraint_evaluator().clone()
    }
}
