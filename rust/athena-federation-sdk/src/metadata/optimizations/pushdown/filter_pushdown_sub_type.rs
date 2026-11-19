use crate::named_enum::named_enum;

named_enum! {
    pub enum FilterPushdownSubType {
        SortedRangeSet => "SORTED_RANGE_SET",
        EquatableValueSet => "EQUATABLE_VALUE_SET",
        AllOrNoneValueSet => "ALL_OR_NONE_VALUE_SET",
        NullableComparison => "NULLABLE_COMPARISON",
    }
}

impl FilterPushdownSubType {
    pub fn sub_type(&self) -> &'static str {
        match self {
            FilterPushdownSubType::SortedRangeSet => "sorted_range_set",
            FilterPushdownSubType::EquatableValueSet => "equatable_range_set",
            FilterPushdownSubType::AllOrNoneValueSet => "all_or_none_value_set",
            FilterPushdownSubType::NullableComparison => "nullable_comparison",
        }
    }
}
