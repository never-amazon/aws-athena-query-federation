use arrow::datatypes::Field;

use super::types::{minor_type_of, MinorType};
use super::value::Value;
use crate::exceptions::{FederationError, Result};

pub trait FieldResolver {
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value>;

    fn get_map_key(&self, _field: &Field, value: &Value) -> Result<Value> {
        Ok(value.clone())
    }

    fn get_map_value(&self, _field: &Field, value: &Value) -> Result<Value> {
        Ok(value.clone())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultFieldResolver;

impl FieldResolver for DefaultFieldResolver {
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value> {
        let minor_type = minor_type_of(field.data_type());
        if let Value::Map(_) = value {
            return Ok(value.map_get(field.name()).cloned().unwrap_or(Value::Null));
        } else if minor_type == MinorType::List {
            if let Value::List(_) = value {
                return Ok(value.clone());
            }
            return Err(FederationError::class_cast(
                value.java_class_name(),
                "java.util.List",
            ));
        }
        Err(FederationError::invalid_input(format!(
            "Expected LIST type but found {minor_type}"
        )))
    }
}

impl<F> FieldResolver for F
where
    F: Fn(&Field, &Value) -> Result<Value>,
{
    fn get_field_value(&self, field: &Field, value: &Value) -> Result<Value> {
        self(field, value)
    }
}
