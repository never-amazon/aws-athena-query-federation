use std::fmt;

use crate::data::Block;

#[derive(Debug, Clone)]
pub struct UserDefinedFunctionResponse {
    records: Block,
    method_name: String,
}

impl UserDefinedFunctionResponse {
    pub fn new(records: Block, method_name: impl Into<String>) -> UserDefinedFunctionResponse {
        UserDefinedFunctionResponse {
            records,
            method_name: method_name.into(),
        }
    }

    pub fn records(&self) -> &Block {
        &self.records
    }

    pub fn method_name(&self) -> &str {
        &self.method_name
    }
}

impl PartialEq for UserDefinedFunctionResponse {
    fn eq(&self, other: &Self) -> bool {
        self.records == other.records && self.method_name == other.method_name
    }
}

impl fmt::Display for UserDefinedFunctionResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "UserDefinedFunctionResponse{{records={}, methodName={}}}",
            self.records, self.method_name
        )
    }
}
