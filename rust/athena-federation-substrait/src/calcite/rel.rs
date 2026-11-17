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
    pub fn new(
        field_index: usize,
        direction: Direction,
        null_direction: NullDirection,
    ) -> RelFieldCollation {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOpKind {
    Union,
    Intersect,
    Minus,
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
        variables_set: Vec<usize>,
    },
    Join {
        left: Box<RelNode>,
        right: Box<RelNode>,
        condition: RexNode,
        join_type: JoinType,
        row_type: RelDataType,
    },
    SetOp {
        kind: SetOpKind,
        all: bool,
        inputs: Vec<RelNode>,
        row_type: RelDataType,
    },
    Project {
        input: Box<RelNode>,
        exprs: Vec<RexNode>,
        row_type: RelDataType,
    },
    Aggregate {
        input: Box<RelNode>,
        group_set: Vec<usize>,
        group_sets: Vec<Vec<usize>>,
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
            | RelNode::Aggregate { row_type, .. }
            | RelNode::Join { row_type, .. }
            | RelNode::SetOp { row_type, .. } => row_type,
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
            RelNode::Join { left, right, .. } => vec![left.as_ref(), right.as_ref()],
            RelNode::SetOp { inputs, .. } => inputs.iter().collect(),
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

pub fn derive_aggregate_row_type(
    input_row_type: &RelDataType,
    group_set: &[usize],
    group_sets: &[Vec<usize>],
    calls: &[AggregateCall],
) -> RelDataType {
    let mut builder = RelDataTypeBuilder::new();
    let fields = input_row_type.get_field_list();
    let mut contained: Vec<String> = Vec::new();
    for &g in group_set {
        if let Some(f) = fields.get(g) {
            contained.push(f.get_name().to_string());
            let in_all = group_sets.iter().all(|set| set.contains(&g));
            let ty = if in_all {
                f.get_type().clone()
            } else {
                f.get_type().with_nullability(true)
            };
            builder.add(f.get_name(), ty);
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

pub fn bit_set_compare(a: &[usize], b: &[usize]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a == b {
        return Ordering::Equal;
    }
    if b.iter().all(|x| a.contains(x)) {
        return Ordering::Less;
    }
    if a.iter().all(|x| b.contains(x)) {
        return Ordering::Greater;
    }
    let mut i = 0;
    loop {
        let n0 = a.get(i).map_or(-1, |&v| v as i64);
        let n1 = b.get(i).map_or(-1, |&v| v as i64);
        let c = n0.cmp(&n1);
        if c != Ordering::Equal || n0 < 0 {
            return c;
        }
        i += 1;
    }
}

pub fn derive_join_row_type(
    left: &RelDataType,
    right: &RelDataType,
    join_type: JoinType,
) -> RelDataType {
    let mut builder = RelDataTypeBuilder::new();
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut add = |f: &super::rel_data_type::RelDataTypeField,
                   nullable: bool,
                   builder: &mut RelDataTypeBuilder| {
        let mut name = f.get_name().to_string();
        if names.contains(&name) {
            let mut j = 0;
            loop {
                let candidate = format!("{}{j}", f.get_name());
                if !names.contains(&candidate) {
                    name = candidate;
                    break;
                }
                j += 1;
            }
        }
        names.insert(name.clone());
        let ty = if nullable {
            f.get_type().with_nullability(true)
        } else {
            f.get_type().clone()
        };
        builder.add(name, ty);
    };
    let left_nullable = matches!(join_type, JoinType::Right | JoinType::Full);
    let right_nullable = matches!(join_type, JoinType::Left | JoinType::Full);
    for f in left.get_field_list() {
        add(f, left_nullable, &mut builder);
    }
    if !matches!(join_type, JoinType::Semi | JoinType::Anti) {
        for f in right.get_field_list() {
            add(f, right_nullable, &mut builder);
        }
    }
    builder.build()
}
