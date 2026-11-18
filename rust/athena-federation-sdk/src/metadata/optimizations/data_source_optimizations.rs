use std::fmt;

use super::pushdown::{
    FilterPushdownSubType, HintsSubtype, LimitPushdownSubType, SubTypeProperties,
    TopNPushdownSubType,
};
use super::OptimizationSubType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataSourceOptimizations {
    SupportsLimitPushdown,
    SupportsTopNPushdown,
    SupportsFilterPushdown,
    SupportsComplexExpressionPushdown,
    DataSourceHints,
}

impl DataSourceOptimizations {
    pub fn values() -> &'static [DataSourceOptimizations] {
        &[
            DataSourceOptimizations::SupportsLimitPushdown,
            DataSourceOptimizations::SupportsTopNPushdown,
            DataSourceOptimizations::SupportsFilterPushdown,
            DataSourceOptimizations::SupportsComplexExpressionPushdown,
            DataSourceOptimizations::DataSourceHints,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            DataSourceOptimizations::SupportsLimitPushdown => "SUPPORTS_LIMIT_PUSHDOWN",
            DataSourceOptimizations::SupportsTopNPushdown => "SUPPORTS_TOP_N_PUSHDOWN",
            DataSourceOptimizations::SupportsFilterPushdown => "SUPPORTS_FILTER_PUSHDOWN",
            DataSourceOptimizations::SupportsComplexExpressionPushdown => {
                "SUPPORTS_COMPLEX_EXPRESSION_PUSHDOWN"
            }
            DataSourceOptimizations::DataSourceHints => "DATA_SOURCE_HINTS",
        }
    }

    pub fn optimization(&self) -> &'static str {
        match self {
            DataSourceOptimizations::SupportsLimitPushdown => "supports_limit_pushdown",
            DataSourceOptimizations::SupportsTopNPushdown => "supports_top_n_pushdown",
            DataSourceOptimizations::SupportsFilterPushdown => "supports_filter_pushdown",
            DataSourceOptimizations::SupportsComplexExpressionPushdown => {
                "supports_complex_expression_pushdown"
            }
            DataSourceOptimizations::DataSourceHints => "data_source_hints",
        }
    }
}

impl fmt::Display for DataSourceOptimizations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Capability {
    Limit(Vec<LimitPushdownSubType>),
    TopN(Vec<TopNPushdownSubType>),
    Filter(Vec<FilterPushdownSubType>),
    ComplexExpression(Vec<SubTypeProperties>),
    Hints(Vec<HintsSubtype>),
}

fn without_properties(
    sub_types: impl IntoIterator<Item = &'static str>,
) -> Vec<OptimizationSubType> {
    sub_types
        .into_iter()
        .map(|s| OptimizationSubType::new(s, Vec::new()))
        .collect()
}

impl Capability {
    pub fn optimization(&self) -> DataSourceOptimizations {
        match self {
            Capability::Limit(_) => DataSourceOptimizations::SupportsLimitPushdown,
            Capability::TopN(_) => DataSourceOptimizations::SupportsTopNPushdown,
            Capability::Filter(_) => DataSourceOptimizations::SupportsFilterPushdown,
            Capability::ComplexExpression(_) => {
                DataSourceOptimizations::SupportsComplexExpressionPushdown
            }
            Capability::Hints(_) => DataSourceOptimizations::DataSourceHints,
        }
    }

    pub fn into_entry(self) -> (String, Vec<OptimizationSubType>) {
        let name = self.optimization().optimization().to_string();
        let sub_types = match self {
            Capability::Limit(s) => {
                without_properties(s.iter().map(LimitPushdownSubType::sub_type))
            }
            Capability::TopN(s) => without_properties(s.iter().map(TopNPushdownSubType::sub_type)),
            Capability::Filter(s) => {
                without_properties(s.iter().map(FilterPushdownSubType::sub_type))
            }
            Capability::Hints(s) => without_properties(s.iter().map(HintsSubtype::sub_type)),
            Capability::ComplexExpression(s) => s
                .into_iter()
                .map(|p| {
                    let (sub_type, properties) = p.into_parts();
                    OptimizationSubType::new(sub_type, properties)
                })
                .collect(),
        };
        (name, sub_types)
    }
}
