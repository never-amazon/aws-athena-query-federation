use std::collections::HashMap;
use std::fmt;

use crate::serde::java_hash::java_hash_map_order;

use indexmap::IndexMap;

use super::expression::FederationExpression;
use super::order_by_field::OrderByField;
use super::query_plan::QueryPlan;
use super::value_set::ValueSet;

pub const DEFAULT_NO_LIMIT: i64 = -1;

#[derive(Debug, Clone, PartialEq)]
pub struct Constraints {
    summary: IndexMap<String, ValueSet>,
    expression: Vec<FederationExpression>,
    order_by_clause: Vec<OrderByField>,
    limit: i64,
    query_passthrough_arguments: HashMap<String, Option<String>>,
    query_plan: Option<QueryPlan>,
}

impl Default for Constraints {
    fn default() -> Self {
        Constraints::empty()
    }
}

impl Constraints {
    pub fn new(
        summary: IndexMap<String, ValueSet>,
        expression: Vec<FederationExpression>,
        order_by_clause: Vec<OrderByField>,
        limit: i64,
        query_passthrough_arguments: HashMap<String, Option<String>>,
        query_plan: Option<QueryPlan>,
    ) -> Constraints {
        Constraints {
            summary,
            expression,
            order_by_clause,
            limit,
            query_passthrough_arguments,
            query_plan,
        }
    }

    pub fn empty() -> Constraints {
        Constraints::new(
            IndexMap::new(),
            Vec::new(),
            Vec::new(),
            DEFAULT_NO_LIMIT,
            HashMap::new(),
            None,
        )
    }

    pub fn summary(&self) -> &IndexMap<String, ValueSet> {
        &self.summary
    }

    pub fn expression(&self) -> &[FederationExpression] {
        &self.expression
    }

    pub fn limit(&self) -> Option<u64> {
        u64::try_from(self.limit).ok().filter(|&limit| limit > 0)
    }

    pub fn wire_limit(&self) -> i64 {
        self.limit
    }

    pub fn order_by_clause(&self) -> &[OrderByField] {
        &self.order_by_clause
    }

    pub fn has_non_empty_order_by_clause(&self) -> bool {
        !self.order_by_clause.is_empty()
    }

    pub fn query_passthrough_arguments(&self) -> &HashMap<String, Option<String>> {
        &self.query_passthrough_arguments
    }

    pub fn query_plan(&self) -> Option<&QueryPlan> {
        self.query_plan.as_ref()
    }

    pub fn is_query_pass_through(&self) -> bool {
        !self.query_passthrough_arguments.is_empty()
    }
}

impl fmt::Display for Constraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let summary: Vec<String> = self
            .summary
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let expression: Vec<String> = self
            .expression
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let order_by: Vec<String> = self
            .order_by_clause
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        let qpt: Vec<String> = java_hash_map_order(self.query_passthrough_arguments.keys())
            .into_iter()
            .map(|k| {
                format!(
                    "{}={}",
                    k,
                    self.query_passthrough_arguments
                        .get(k)
                        .and_then(|v| v.as_deref())
                        .unwrap_or("null")
                )
            })
            .collect();
        write!(
            f,
            "Constraints{{summary={{{}}}expression=[{}]orderByClause=[{}]limit={}queryPassthroughArguments={{{}}}queryPlan={}}}",
            summary.join(", "),
            expression.join(", "),
            order_by.join(", "),
            self.limit,
            qpt.join(", "),
            self.query_plan.as_ref().map_or_else(|| "null".into(), std::string::ToString::to_string)
        )
    }
}
