#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HintsSubtype {
    NonDefaultCollate,
}

impl HintsSubtype {
    pub fn values() -> &'static [HintsSubtype] {
        &[
            HintsSubtype::NonDefaultCollate,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            HintsSubtype::NonDefaultCollate => "NON_DEFAULT_COLLATE",
        }
    }

    pub fn get_sub_type(&self) -> &'static str {
        match self {
            HintsSubtype::NonDefaultCollate => "non_default_collate",
        }
    }
}
