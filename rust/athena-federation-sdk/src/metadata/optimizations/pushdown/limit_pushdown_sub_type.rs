#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitPushdownSubType {
    IntegerConstant,
}

impl LimitPushdownSubType {
    pub fn values() -> &'static [LimitPushdownSubType] {
        &[
            LimitPushdownSubType::IntegerConstant,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            LimitPushdownSubType::IntegerConstant => "INTEGER_CONSTANT",
        }
    }

    pub fn get_sub_type(&self) -> &'static str {
        match self {
            LimitPushdownSubType::IntegerConstant => "integer_constant",
        }
    }
}
