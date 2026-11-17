mod arrow_value_projector_impl;
mod list_arrow_value_projector;
mod projector_utils;
mod simple_arrow_value_projector;
mod struct_arrow_value_projector;

pub use arrow_value_projector_impl::{create_value_projection, Projection};
pub use list_arrow_value_projector::ListArrowValueProjector;
pub use projector_utils::ProjectorUtils;
pub use simple_arrow_value_projector::SimpleArrowValueProjector;
pub use struct_arrow_value_projector::StructArrowValueProjector;

use crate::data::value::Value;
use crate::exceptions::Result;

pub trait ArrowValueProjector {
    fn project(&self, pos: usize) -> Result<Value>;
}
