use std::fmt;

use athena_federation_sdk::data::{arrow_type_to_string, ArrowType, Value};
use athena_federation_sdk::Result;

use crate::validate::null_pointer;

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAndValue {
    arrow_type: ArrowType,
    value: Value,
}

impl TypeAndValue {
    pub fn new(arrow_type: ArrowType, value: Value) -> Result<TypeAndValue> {
        if value.is_null() {
            return Err(null_pointer("value is null"));
        }
        Ok(TypeAndValue { arrow_type, value })
    }

    pub fn get_type(&self) -> &ArrowType {
        &self.arrow_type
    }

    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl fmt::Display for TypeAndValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TypeAndValue{{type={}, value={}}}",
            arrow_type_to_string(&self.arrow_type),
            self.value.java_to_string()
        )
    }
}
