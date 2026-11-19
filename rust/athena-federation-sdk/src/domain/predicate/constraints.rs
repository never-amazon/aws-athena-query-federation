use std::collections::HashMap;
use std::fmt;

use indexmap::IndexMap;

use crate::domain::predicate::value_set::ValueSet;
use crate::domain::predicate::FederationExpression;
use crate::serde::java_hash::java_hash_map_order;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    AscNullsFirst,
    AscNullsLast,
    DescNullsFirst,
    DescNullsLast,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::AscNullsFirst,
        Direction::AscNullsLast,
        Direction::DescNullsFirst,
        Direction::DescNullsLast,
    ];

    pub fn is_ascending(&self) -> bool {
        matches!(self, Direction::AscNullsFirst | Direction::AscNullsLast)
    }

    pub fn is_nulls_first(&self) -> bool {
        matches!(self, Direction::AscNullsFirst | Direction::DescNullsFirst)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Direction::AscNullsFirst => "ASC_NULLS_FIRST",
            Direction::AscNullsLast => "ASC_NULLS_LAST",
            Direction::DescNullsFirst => "DESC_NULLS_FIRST",
            Direction::DescNullsLast => "DESC_NULLS_LAST",
        }
    }

    pub fn from_name(name: &str) -> Option<Direction> {
        Direction::ALL.into_iter().find(|d| d.as_str() == name)
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}",
            if self.is_ascending() { "ASC" } else { "DESC" },
            if self.is_nulls_first() {
                "NULLS FIRST"
            } else {
                "NULLS LAST"
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderByField {
    column_name: String,
    direction: Direction,
}

impl OrderByField {
    pub fn new(column_name: impl Into<String>, direction: Direction) -> OrderByField {
        OrderByField {
            column_name: column_name.into(),
            direction,
        }
    }

    pub fn column_name(&self) -> &str {
        &self.column_name
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }
}

impl fmt::Display for OrderByField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OrderByField[columnName={}, direction={}]",
            self.column_name,
            self.direction.as_str()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QueryPlan {
    substrait_version: Option<String>,
    substrait_plan: String,
}

impl QueryPlan {
    pub fn new(substrait_version: Option<String>, substrait_plan: impl Into<String>) -> QueryPlan {
        QueryPlan {
            substrait_version,
            substrait_plan: substrait_plan.into(),
        }
    }

    pub fn substrait_version(&self) -> Option<&str> {
        self.substrait_version.as_deref()
    }

    pub fn substrait_plan(&self) -> &str {
        &self.substrait_plan
    }
}

impl fmt::Display for QueryPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "QueryPlan{{substraitVersion={}, substraitPlan={}}}",
            self.substrait_version.as_deref().unwrap_or("null"),
            self.substrait_plan
        )
    }
}
