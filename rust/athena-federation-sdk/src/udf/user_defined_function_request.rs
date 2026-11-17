use std::fmt;

use arrow::datatypes::Schema;

use super::UserDefinedFunctionType;
use crate::data::Block;
use crate::request::FederatedIdentity;

#[derive(Debug, Clone)]
pub struct UserDefinedFunctionRequest {
    identity: Option<FederatedIdentity>,
    input_records: Block,
    output_schema: Schema,
    method_name: String,
    function_type: UserDefinedFunctionType,
}

impl UserDefinedFunctionRequest {
    pub fn new(
        identity: Option<FederatedIdentity>,
        input_records: Block,
        output_schema: Schema,
        method_name: impl Into<String>,
        function_type: UserDefinedFunctionType,
    ) -> UserDefinedFunctionRequest {
        UserDefinedFunctionRequest {
            identity,
            input_records,
            output_schema,
            method_name: method_name.into(),
            function_type,
        }
    }

    pub fn identity(&self) -> Option<&FederatedIdentity> {
        self.identity.as_ref()
    }

    pub fn input_records(&self) -> &Block {
        &self.input_records
    }

    pub fn output_schema(&self) -> &Schema {
        &self.output_schema
    }

    pub fn method_name(&self) -> &str {
        &self.method_name
    }

    pub fn function_type(&self) -> UserDefinedFunctionType {
        self.function_type
    }
}

impl PartialEq for UserDefinedFunctionRequest {
    fn eq(&self, other: &Self) -> bool {
        self.input_records == other.input_records
            && self.output_schema == other.output_schema
            && self.method_name == other.method_name
            && self.function_type == other.function_type
    }
}

impl fmt::Display for UserDefinedFunctionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "UserDefinedFunctionRequest{{inputRecords={}, outputSchema={}, methodName={}, functionType={}}}",
            self.input_records, self.output_schema, self.method_name, self.function_type
        )
    }
}
