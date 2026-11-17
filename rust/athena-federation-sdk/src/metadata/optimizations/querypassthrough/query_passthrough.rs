use super::super::OptimizationSubType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryPassthrough {
    QueryPassthroughSchema,
    QueryPassthroughName,
    QueryPassthroughArguments,
}

impl QueryPassthrough {
    pub fn values() -> &'static [QueryPassthrough] {
        &[
            QueryPassthrough::QueryPassthroughSchema,
            QueryPassthrough::QueryPassthroughName,
            QueryPassthrough::QueryPassthroughArguments,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            QueryPassthrough::QueryPassthroughSchema => "QUERY_PASSTHROUGH_SCHEMA",
            QueryPassthrough::QueryPassthroughName => "QUERY_PASSTHROUGH_NAME",
            QueryPassthrough::QueryPassthroughArguments => "QUERY_PASSTHROUGH_ARGUMENTS",
        }
    }

    pub fn get_value(&self) -> &'static str {
        match self {
            QueryPassthrough::QueryPassthroughSchema => "query_passthrough_schema",
            QueryPassthrough::QueryPassthroughName => "query_passthrough_name",
            QueryPassthrough::QueryPassthroughArguments => "query_passthrough_arguments",
        }
    }

    pub fn with_schema(&self, schema: &str) -> OptimizationSubType {
        OptimizationSubType::new(self.get_value(), vec![schema.to_string()])
    }

    pub fn with_name(&self, name: &str) -> OptimizationSubType {
        OptimizationSubType::new(self.get_value(), vec![name.to_string()])
    }

    pub fn with_arguments(&self, arguments: Vec<String>) -> OptimizationSubType {
        OptimizationSubType::new(self.get_value(), arguments)
    }
}
