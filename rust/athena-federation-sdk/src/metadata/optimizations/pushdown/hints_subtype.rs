use crate::named_enum::named_enum;

named_enum! {
    pub enum HintsSubtype {
        NonDefaultCollate => "NON_DEFAULT_COLLATE",
    }
}

impl HintsSubtype {
    pub fn sub_type(&self) -> &'static str {
        match self {
            HintsSubtype::NonDefaultCollate => "non_default_collate",
        }
    }
}
