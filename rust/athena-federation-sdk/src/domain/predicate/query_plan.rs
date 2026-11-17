use std::fmt;

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
