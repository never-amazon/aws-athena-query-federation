#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterPushdownSubType {
    SortedRangeSet,
    EquatableValueSet,
    AllOrNoneValueSet,
    NullableComparison,
}

impl FilterPushdownSubType {
    pub fn values() -> &'static [FilterPushdownSubType] {
        &[
            FilterPushdownSubType::SortedRangeSet,
            FilterPushdownSubType::EquatableValueSet,
            FilterPushdownSubType::AllOrNoneValueSet,
            FilterPushdownSubType::NullableComparison,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            FilterPushdownSubType::SortedRangeSet => "SORTED_RANGE_SET",
            FilterPushdownSubType::EquatableValueSet => "EQUATABLE_VALUE_SET",
            FilterPushdownSubType::AllOrNoneValueSet => "ALL_OR_NONE_VALUE_SET",
            FilterPushdownSubType::NullableComparison => "NULLABLE_COMPARISON",
        }
    }

    pub fn sub_type(&self) -> &'static str {
        match self {
            FilterPushdownSubType::SortedRangeSet => "sorted_range_set",
            FilterPushdownSubType::EquatableValueSet => "equatable_range_set",
            FilterPushdownSubType::AllOrNoneValueSet => "all_or_none_value_set",
            FilterPushdownSubType::NullableComparison => "nullable_comparison",
        }
    }
}
