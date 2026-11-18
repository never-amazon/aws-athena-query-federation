use std::sync::Arc;

use arrow::datatypes::{Field, FieldRef};

use super::types::{get_children, with_children, ArrowType};

pub fn remap_arrow_types_within_field(
    input_field: &Field,
    arrow_type_mapper: &dyn Fn(&ArrowType) -> ArrowType,
) -> Field {
    let updated_children: Vec<FieldRef> = get_children(input_field)
        .iter()
        .map(|f| Arc::new(remap_arrow_types_within_field(f, arrow_type_mapper)))
        .collect();
    let new_type = arrow_type_mapper(input_field.data_type());
    let new_type = if updated_children.is_empty() {
        new_type
    } else {
        with_children(&new_type, updated_children)
    };
    Field::new(
        input_field.name().clone(),
        new_type,
        input_field.is_nullable(),
    )
    .with_metadata(input_field.metadata().clone())
}
