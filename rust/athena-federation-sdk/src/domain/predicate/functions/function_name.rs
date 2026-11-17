use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FunctionName {
    function_name: String,
}

impl FunctionName {
    pub fn new(function_name: impl Into<String>) -> FunctionName {
        FunctionName {
            function_name: function_name.into(),
        }
    }

    pub fn get_function_name(&self) -> &str {
        &self.function_name
    }
}

impl fmt::Display for FunctionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FunctionName{{functionName={}}}", self.function_name)
    }
}
