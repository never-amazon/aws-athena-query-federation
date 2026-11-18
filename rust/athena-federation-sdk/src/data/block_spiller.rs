use std::sync::Arc;

use async_trait::async_trait;

use super::block::Block;
use crate::domain::predicate::ConstraintEvaluator;
use crate::domain::spill::SpillLocation;
use crate::exceptions::Result;

pub type RowWriter<'a> = dyn FnMut(&mut Block, usize) -> Result<usize> + Send + 'a;

#[async_trait]
pub trait BlockSpiller: Send + Sync {
    async fn write_rows(&self, row_writer: &mut RowWriter<'_>) -> Result<()>;

    fn constraint_evaluator(&self) -> Arc<ConstraintEvaluator>;

    async fn spilled(&self) -> Result<bool>;

    async fn get_block(&self) -> Result<Block>;

    async fn get_spill_locations(&self) -> Result<Vec<SpillLocation>>;

    async fn close(&self);
}
