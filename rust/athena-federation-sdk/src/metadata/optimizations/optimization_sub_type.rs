use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OptimizationSubType {
    sub_type: String,
    properties: Vec<String>,
}

impl OptimizationSubType {
    pub fn new(sub_type: impl Into<String>, properties: Vec<String>) -> OptimizationSubType {
        OptimizationSubType {
            sub_type: sub_type.into(),
            properties,
        }
    }

    pub fn get_sub_type(&self) -> &str {
        &self.sub_type
    }

    pub fn get_properties(&self) -> &[String] {
        &self.properties
    }
}

impl fmt::Display for OptimizationSubType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OptimizationSubType{{optimization={}, properties=[{}]}}",
            self.sub_type,
            self.properties.join(", ")
        )
    }
}
