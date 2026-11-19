use super::super::OptimizationSubType;
use crate::named_enum::named_enum;

named_enum! {
    pub enum QueryPassthrough {
        QueryPassthroughSchema => "QUERY_PASSTHROUGH_SCHEMA",
        QueryPassthroughName => "QUERY_PASSTHROUGH_NAME",
        QueryPassthroughArguments => "QUERY_PASSTHROUGH_ARGUMENTS",
    }
}

impl QueryPassthrough {
    pub fn value(&self) -> &'static str {
        match self {
            QueryPassthrough::QueryPassthroughSchema => "query_passthrough_schema",
            QueryPassthrough::QueryPassthroughName => "query_passthrough_name",
            QueryPassthrough::QueryPassthroughArguments => "query_passthrough_arguments",
        }
    }

    pub fn with_schema(&self, schema: &str) -> OptimizationSubType {
        OptimizationSubType::new(self.value(), vec![schema.to_string()])
    }

    pub fn with_name(&self, name: &str) -> OptimizationSubType {
        OptimizationSubType::new(self.value(), vec![name.to_string()])
    }

    pub fn with_arguments(&self, arguments: Vec<String>) -> OptimizationSubType {
        OptimizationSubType::new(self.value(), arguments)
    }
}
