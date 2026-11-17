use std::sync::Arc;

use super::block::Block;
use super::block_writer::BlockWriter;
use crate::domain::predicate::ConstraintEvaluator;
use crate::domain::spill::SpillLocation;
use crate::exceptions::Result;

pub trait BlockSpiller: BlockWriter {
    fn spilled(&self) -> bool;

    fn get_block(&self) -> Result<&Block>;

    fn get_spill_locations(&self) -> Vec<SpillLocation>;

    fn close(&mut self);

    fn get_spiller_constraint_evaluator(&self) -> Arc<ConstraintEvaluator> {
        self.get_constraint_evaluator()
    }
}
