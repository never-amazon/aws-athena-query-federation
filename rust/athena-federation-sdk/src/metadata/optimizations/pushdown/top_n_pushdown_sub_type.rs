use crate::named_enum::named_enum;

named_enum! {
    pub enum TopNPushdownSubType {
        SupportsOrderBy => "SUPPORTS_ORDER_BY",
    }
}

impl TopNPushdownSubType {
    pub fn sub_type(&self) -> &'static str {
        match self {
            TopNPushdownSubType::SupportsOrderBy => "SUPPORTS_ORDER_BY",
        }
    }
}
