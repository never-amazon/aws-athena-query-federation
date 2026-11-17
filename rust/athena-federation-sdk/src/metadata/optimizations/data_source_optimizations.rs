use std::fmt;

use super::pushdown::PushdownSubTypes;
use super::OptimizationSubType;
use crate::exceptions::{FederationError, Result};

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

    pub fn with_supported_sub_types(
        &self,
        sub_types_list: &[PushdownSubTypes],
    ) -> Result<(String, Vec<OptimizationSubType>)> {
        let (valid, message) = match self {
            DataSourceOptimizations::SupportsLimitPushdown => (
                sub_types_list
                    .iter()
                    .all(|s| matches!(s, PushdownSubTypes::Limit(_))),
                "Limit Pushdown Optimization must contain valid pushdown subtypes.",
            ),
            DataSourceOptimizations::SupportsTopNPushdown => (
                sub_types_list
                    .iter()
                    .all(|s| matches!(s, PushdownSubTypes::TopN(_))),
                "TopN Pushdown Optimization must contain valid pushdown subtypes.",
            ),
            DataSourceOptimizations::SupportsFilterPushdown => (
                sub_types_list
                    .iter()
                    .all(|s| matches!(s, PushdownSubTypes::Filter(_))),
                "Filter Pushdown Optimization must contain valid pushdown subtypes.",
            ),
            DataSourceOptimizations::SupportsComplexExpressionPushdown => (
                sub_types_list.iter().all(|s| {
                    matches!(
                        s,
                        PushdownSubTypes::ComplexExpression(_)
                            | PushdownSubTypes::ComplexExpressionProperties(_)
                    )
                }),
                "Complex Expression Pushdown Optimization must contain valid pushdown subtypes.",
            ),
            DataSourceOptimizations::DataSourceHints => (
                sub_types_list
                    .iter()
                    .all(|s| matches!(s, PushdownSubTypes::Hints(_))),
                "Data Source Hints  must contain valid data source hint subtypes.",
            ),
        };
        if !valid {
            return Err(FederationError::invalid_input(message));
        }
        Ok((
            self.optimization().to_string(),
            sub_types_list
                .iter()
                .map(|s| OptimizationSubType::new(s.sub_type(), s.properties()))
                .collect(),
        ))
    }
}

impl fmt::Display for DataSourceOptimizations {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
