use std::sync::Arc;

use crate::data::Value;
use crate::exceptions::Result;

pub type ConstraintProjector = Arc<dyn Fn(&Value) -> Result<bool> + Send + Sync>;
