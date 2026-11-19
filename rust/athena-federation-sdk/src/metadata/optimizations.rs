use std::fmt;

use crate::exceptions::{FederationError, Result};
use crate::named_enum::named_enum;

named_enum! {
    pub enum DataSourceOptimizations {
        SupportsLimitPushdown => "SUPPORTS_LIMIT_PUSHDOWN",
        SupportsTopNPushdown => "SUPPORTS_TOP_N_PUSHDOWN",
        SupportsFilterPushdown => "SUPPORTS_FILTER_PUSHDOWN",
        SupportsComplexExpressionPushdown => "SUPPORTS_COMPLEX_EXPRESSION_PUSHDOWN",
        DataSourceHints => "DATA_SOURCE_HINTS",
    }
}

impl DataSourceOptimizations {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OptimizationSubType {
    sub_type: String,
    properties: Vec<String>,
}

impl OptimizationSubType {
    pub fn new(sub_type: impl Into<String>, properties: Vec<String>) -> OptimizationSubType {
        OptimizationSubType {
            sub_type: sub_type.into(),
            properties,
        }
    }

    pub fn sub_type(&self) -> &str {
        &self.sub_type
    }

    pub fn properties(&self) -> &[String] {
        &self.properties
    }
}

impl fmt::Display for OptimizationSubType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OptimizationSubType{{optimization={}, properties=[{}]}}",
            self.sub_type,
            self.properties.join(", ")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SubTypeProperties {
    property_name: String,
    property_values: Vec<String>,
}

impl SubTypeProperties {
    pub fn new(
        property_name: impl Into<String>,
        property_values: Vec<String>,
    ) -> SubTypeProperties {
        SubTypeProperties {
            property_name: property_name.into(),
            property_values,
        }
    }

    pub fn sub_type(&self) -> &str {
        &self.property_name
    }

    pub fn properties(&self) -> &[String] {
        &self.property_values
    }

    pub fn into_parts(self) -> (String, Vec<String>) {
        (self.property_name, self.property_values)
    }
}

named_enum! {
    pub enum ComplexExpressionPushdownSubType {
        SupportedFunctionExpressionTypes => "SUPPORTED_FUNCTION_EXPRESSION_TYPES",
    }
}

impl ComplexExpressionPushdownSubType {
    pub fn sub_type(&self) -> &'static str {
        "supported_function_expression_types"
    }

    pub fn with_sub_type_properties(&self, properties: &[&str]) -> Result<SubTypeProperties> {
        if properties.is_empty() {
            return Err(FederationError::invalid_input(
                "Connectors that support function expressions must provide a list of supported functions. See Documentation for more details",
            ));
        }
        Ok(SubTypeProperties::new(
            self.sub_type(),
            properties
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        ))
    }
}

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
