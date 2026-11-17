use super::operators::SqlOperator;
use super::rel_data_type::{RelDataType, RelDataTypeBuilder};
use super::rex::{Direction, NullDirection, RexLiteral, RexNode};

#[derive(Debug, Clone, PartialEq)]
pub struct RelFieldCollation {
    pub field_index: usize,
    pub direction: Direction,
    pub null_direction: NullDirection,
}

impl RelFieldCollation {
    pub fn new(field_index: usize, direction: Direction, null_direction: NullDirection) -> RelFieldCollation {
        RelFieldCollation {
            field_index,
            direction,
            null_direction,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AggregateCall {
    pub op: &'static SqlOperator,
    pub distinct: bool,
    pub approximate: bool,
    pub args: Vec<usize>,
    pub filter_arg: Option<usize>,
    pub collation: Vec<RelFieldCollation>,
    pub ty: RelDataType,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RelNode {
    TableScan {
        qualified_name: Vec<String>,
        row_type: RelDataType,
    },
    Values {
        row_type: RelDataType,
        tuples: Vec<Vec<RexLiteral>>,
    },
    Filter {
        input: Box<RelNode>,
        condition: RexNode,
    },
    Project {
        input: Box<RelNode>,
        exprs: Vec<RexNode>,
        row_type: RelDataType,
    },
    Aggregate {
        input: Box<RelNode>,
        group_set: Vec<usize>,
        agg_calls: Vec<AggregateCall>,
        row_type: RelDataType,
    },
    Sort {
        input: Box<RelNode>,
        collation: Vec<RelFieldCollation>,
        offset: Option<RexNode>,
        fetch: Option<RexNode>,
    },
}

impl RelNode {
    pub fn row_type(&self) -> &RelDataType {
        match self {
            RelNode::TableScan { row_type, .. }
            | RelNode::Values { row_type, .. }
            | RelNode::Project { row_type, .. }
            | RelNode::Aggregate { row_type, .. } => row_type,
            RelNode::Filter { input, .. } | RelNode::Sort { input, .. } => input.row_type(),
        }
    }

    pub fn inputs(&self) -> Vec<&RelNode> {
        match self {
            RelNode::TableScan { .. } | RelNode::Values { .. } => Vec::new(),
            RelNode::Filter { input, .. }
            | RelNode::Project { input, .. }
            | RelNode::Aggregate { input, .. }
            | RelNode::Sort { input, .. } => vec![input.as_ref()],
        }
    }

    pub fn input(&self) -> Option<&RelNode> {
        self.inputs().into_iter().next()
    }

    pub fn is_project(&self) -> bool {
        matches!(self, RelNode::Project { .. })
    }

    pub fn is_sort(&self) -> bool {
        matches!(self, RelNode::Sort { .. })
    }

    pub fn project_contains_over(&self) -> bool {
        match self {
            RelNode::Project { exprs, .. } => exprs.iter().any(RexNode::contains_over),
            _ => false,
        }
    }

}

pub fn derive_aggregate_row_type(input_row_type: &RelDataType, group_set: &[usize], calls: &[AggregateCall]) -> RelDataType {
    let mut builder = RelDataTypeBuilder::new();
    let fields = input_row_type.get_field_list();
    let mut contained: Vec<String> = Vec::new();
    for &g in group_set {
        if let Some(f) = fields.get(g) {
            contained.push(f.get_name().to_string());
            builder.add(f.get_name(), f.get_type().clone());
        }
    }
    for (i, call) in calls.iter().enumerate() {
        let base = match &call.name {
            Some(n) => n.clone(),
            None => format!("$f{}", group_set.len() + i),
        };
        let mut name = base.clone();
        let mut j = 0;
        while contained.contains(&name) {
            name = format!("{base}_{j}");
            j += 1;
        }
        contained.push(name.clone());
        builder.add(name, call.ty.clone());
    }
    builder.build()
}
