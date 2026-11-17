use super::{
    ComplexExpressionPushdownSubType, ComplexExpressionSubTypeProperties, FilterPushdownSubType,
    HintsSubtype, LimitPushdownSubType, SubTypeProperties, TopNPushdownSubType,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PushdownSubTypes {
    Limit(LimitPushdownSubType),
    TopN(TopNPushdownSubType),
    Filter(FilterPushdownSubType),
    ComplexExpression(ComplexExpressionPushdownSubType),
    ComplexExpressionProperties(ComplexExpressionSubTypeProperties),
    Hints(HintsSubtype),
    Properties(SubTypeProperties),
}

impl PushdownSubTypes {
    pub fn sub_type(&self) -> String {
        match self {
            PushdownSubTypes::Limit(s) => s.sub_type().to_string(),
            PushdownSubTypes::TopN(s) => s.sub_type().to_string(),
            PushdownSubTypes::Filter(s) => s.sub_type().to_string(),
            PushdownSubTypes::ComplexExpression(s) => s.sub_type().to_string(),
            PushdownSubTypes::ComplexExpressionProperties(s) => s.sub_type().to_string(),
            PushdownSubTypes::Hints(s) => s.sub_type().to_string(),
            PushdownSubTypes::Properties(s) => s.sub_type().to_string(),
        }
    }

    pub fn properties(&self) -> Vec<String> {
        match self {
            PushdownSubTypes::ComplexExpressionProperties(s) => s.properties().to_vec(),
            PushdownSubTypes::Properties(s) => s.properties().to_vec(),
            _ => Vec::new(),
        }
    }
}

macro_rules! from_sub_type {
    ($variant:ident, $t:ty) => {
        impl From<$t> for PushdownSubTypes {
            fn from(value: $t) -> Self {
                PushdownSubTypes::$variant(value)
            }
        }
    };
}

from_sub_type!(Limit, LimitPushdownSubType);
from_sub_type!(TopN, TopNPushdownSubType);
from_sub_type!(Filter, FilterPushdownSubType);
from_sub_type!(ComplexExpression, ComplexExpressionPushdownSubType);
from_sub_type!(
    ComplexExpressionProperties,
    ComplexExpressionSubTypeProperties
);
from_sub_type!(Hints, HintsSubtype);
from_sub_type!(Properties, SubTypeProperties);
