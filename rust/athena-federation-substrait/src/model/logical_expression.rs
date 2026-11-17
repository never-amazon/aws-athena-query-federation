use crate::model::column_predicate::ColumnPredicate;
use crate::model::substrait_operator::SubstraitOperator;

#[derive(Debug, Clone, PartialEq)]
pub struct LogicalExpression {
    operator: SubstraitOperator,
    children: Option<Vec<LogicalExpression>>,
    leaf_predicate: Option<ColumnPredicate>,
}

impl LogicalExpression {
    pub fn new(operator: SubstraitOperator, children: Vec<LogicalExpression>) -> LogicalExpression {
        LogicalExpression {
            operator,
            children: Some(children),
            leaf_predicate: None,
        }
    }

    pub fn from_leaf(leaf_predicate: ColumnPredicate) -> LogicalExpression {
        LogicalExpression {
            operator: leaf_predicate.operator(),
            children: None,
            leaf_predicate: Some(leaf_predicate),
        }
    }

    pub fn operator(&self) -> SubstraitOperator {
        self.operator
    }

    pub fn children(&self) -> Option<&[LogicalExpression]> {
        self.children.as_deref()
    }

    pub fn leaf_predicate(&self) -> Option<&ColumnPredicate> {
        self.leaf_predicate.as_ref()
    }

    pub fn is_leaf(&self) -> bool {
        self.leaf_predicate.is_some()
    }

    pub fn has_complex_logic(&self) -> bool {
        if self.is_leaf() {
            return false;
        }
        self.operator == SubstraitOperator::And || self.operator == SubstraitOperator::Or
    }
}
