use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserDefinedFunctionType {
    SCALAR,
}

impl UserDefinedFunctionType {
    pub fn name(&self) -> &'static str {
        "SCALAR"
    }

    pub fn values() -> &'static [UserDefinedFunctionType] {
        &[UserDefinedFunctionType::SCALAR]
    }

    pub fn value_of(name: &str) -> Option<UserDefinedFunctionType> {
        match name {
            "SCALAR" => Some(UserDefinedFunctionType::SCALAR),
            _ => None,
        }
    }
}

impl fmt::Display for UserDefinedFunctionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
