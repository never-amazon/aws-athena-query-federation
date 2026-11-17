#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TopNPushdownSubType {
    SupportsOrderBy,
}

impl TopNPushdownSubType {
    pub fn values() -> &'static [TopNPushdownSubType] {
        &[TopNPushdownSubType::SupportsOrderBy]
    }

    pub fn name(&self) -> &'static str {
        match self {
            TopNPushdownSubType::SupportsOrderBy => "SUPPORTS_ORDER_BY",
        }
    }

    pub fn get_sub_type(&self) -> &'static str {
        match self {
            TopNPushdownSubType::SupportsOrderBy => "SUPPORTS_ORDER_BY",
        }
    }
}
