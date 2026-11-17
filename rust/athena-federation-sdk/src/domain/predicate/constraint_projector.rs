use crate::data::Value;
use crate::exceptions::Result;

pub trait ConstraintProjector: Send + Sync {
    fn apply(&self, value: &Value) -> Result<bool>;
}

impl<F> ConstraintProjector for F
where
    F: Fn(&Value) -> Result<bool> + Send + Sync,
{
    fn apply(&self, value: &Value) -> Result<bool> {
        self(value)
    }
}
