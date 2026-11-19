use crate::named_enum::named_enum;

named_enum! {
    pub enum LimitPushdownSubType {
        IntegerConstant => "INTEGER_CONSTANT",
    }
}

impl LimitPushdownSubType {
    pub fn sub_type(&self) -> &'static str {
        match self {
            LimitPushdownSubType::IntegerConstant => "integer_constant",
        }
    }
}
