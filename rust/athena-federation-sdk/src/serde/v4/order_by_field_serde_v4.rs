use crate::domain::predicate::{Direction, OrderByField};
use crate::exceptions::{FederationError, Result};
use crate::serde::base::{
    deserialize_nullable, get_next_string_field, require_non_null, serialize_nullable,
};
use crate::serde::json::{JsonGenerator, JsonParser};

const COLUMN_NAME_FIELD: &str = "columnName";
const DIRECTION_FIELD: &str = "direction";

pub fn serialize(order_by_field: Option<&OrderByField>, jgen: &mut JsonGenerator) -> Result<()> {
    serialize_nullable(jgen, order_by_field, do_serialize)
}

pub fn do_serialize(order_by_field: &OrderByField, jgen: &mut JsonGenerator) -> Result<()> {
    jgen.write_string_field(COLUMN_NAME_FIELD, Some(order_by_field.get_column_name()))?;
    jgen.write_string_field(DIRECTION_FIELD, Some(order_by_field.get_direction().name()))
}

pub fn deserialize(jparser: &mut JsonParser<'_>) -> Result<Option<OrderByField>> {
    deserialize_nullable(jparser, do_deserialize)
}

pub fn do_deserialize(jparser: &mut JsonParser<'_>) -> Result<OrderByField> {
    let column_name = get_next_string_field(jparser, COLUMN_NAME_FIELD)?;
    let direction = get_next_string_field(jparser, DIRECTION_FIELD)?;
    let direction_enum = match direction.as_deref() {
        Some("ASC_NULLS_FIRST") => Direction::ASC_NULLS_FIRST,
        Some("ASC_NULLS_LAST") => Direction::ASC_NULLS_LAST,
        Some("DESC_NULLS_FIRST") => Direction::DESC_NULLS_FIRST,
        Some("DESC_NULLS_LAST") => Direction::DESC_NULLS_LAST,
        Some(other) => {
            return Err(FederationError::invalid_input(format!(
                "Unexpected value: {other}"
            )))
        }
        None => {
            return Err(FederationError::NullPointer(
                "Cannot invoke \"String.hashCode()\" because \"direction\" is null".into(),
            ))
        }
    };
    Ok(OrderByField::new(
        require_non_null(column_name, "columnName is null")?,
        direction_enum,
    ))
}
