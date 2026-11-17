use std::sync::Arc;

use super::block::Block;
use crate::domain::predicate::ConstraintEvaluator;
use crate::exceptions::Result;

pub trait RowWriter {
    fn write_rows(&mut self, block: &mut Block, row_num: usize) -> Result<usize>;
}

impl<F> RowWriter for F
where
    F: FnMut(&mut Block, usize) -> Result<usize>,
{
    fn write_rows(&mut self, block: &mut Block, row_num: usize) -> Result<usize> {
        self(block, row_num)
    }
}

pub trait BlockWriter {
    fn write_rows(&mut self, row_writer: &mut dyn RowWriter) -> Result<()>;

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator>;
}
