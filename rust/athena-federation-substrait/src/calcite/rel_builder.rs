use std::collections::HashSet;

use super::rel::{
    bit_set_compare, derive_aggregate_row_type, derive_join_row_type, AggregateCall, JoinType,
    RelFieldCollation, RelNode, SetOpKind,
};
use super::rel_data_type::{RelDataType, RelDataTypeBuilder};
use super::rex::{Direction, LiteralValue, NullDirection, RexBuilder, RexLiteral, RexNode};
use super::rex_simplify::RexSimplify;
use super::sql_type_name::SqlTypeName;
use crate::error::{Result, SubstraitError};

#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub expr: RexNode,
    pub direction: Direction,
    pub null_direction: Option<NullDirection>,
}

#[derive(Debug, Clone, Copy)]
pub struct RelBuilder {
    pub rex_builder: RexBuilder,
    simplifier: RexSimplify,
    bloat: usize,
}

fn resolve_group_sets(
    extra: &[RexNode],
    group_set: &[usize],
    group_expr_lists: Option<&[Vec<RexNode>]>,
) -> Result<Vec<Vec<usize>>> {
    let Some(lists) = group_expr_lists else {
        return Ok(vec![group_set.to_vec()]);
    };
    let mut sets = Vec::with_capacity(lists.len());
    for list in lists {
        let mut set = Vec::with_capacity(list.len());
        for e in list {
            let idx = extra.iter().position(|x| x == e).ok_or_else(|| {
                SubstraitError::illegal_argument("group set element must be a subset of group key")
            })?;
            set.push(idx);
        }
        set.sort_unstable();
        set.dedup();
        sets.push(set);
    }
    sets.sort_by(|a, b| bit_set_compare(a, b));
    if sets.windows(2).any(|w| w[0] == w[1]) {
        return Err(SubstraitError::unsupported(
            "Aggregates with duplicate grouping sets are not supported by the Rust Substrait converter",
        ));
    }
    Ok(sets)
}

fn prune_project(
    r: RelNode,
    group_set: &[usize],
    group_sets: &mut [Vec<usize>],
    calls: &mut [AggregateCall],
) -> (RelNode, Vec<usize>) {
    let mut group_after = group_set.to_vec();
    let r = match r {
        RelNode::Project {
            input: p_input,
            exprs,
            row_type,
        } => {
            let mut used: Vec<usize> = group_set.to_vec();
            for c in calls.iter() {
                used.extend(c.args.iter().copied());
                if let Some(f) = c.filter_arg {
                    used.push(f);
                }
                used.extend(c.collation.iter().map(|fc| fc.field_index));
            }
            used.sort_unstable();
            used.dedup();
            if used.is_empty() {
                *p_input
            } else if used.len() < row_type.get_field_count() {
                let map = |source: usize| used.iter().position(|u| *u == source).unwrap_or(source);
                group_after = group_set.iter().map(|g| map(*g)).collect();
                for set in group_sets.iter_mut() {
                    *set = set.iter().map(|g| map(*g)).collect();
                }
                group_sets.sort_by(|a, b| bit_set_compare(a, b));
                for c in calls.iter_mut() {
                    c.args = c.args.iter().map(|a| map(*a)).collect();
                    c.filter_arg = c.filter_arg.map(map);
                    for fc in &mut c.collation {
                        fc.field_index = map(fc.field_index);
                    }
                }
                let mut builder = RelDataTypeBuilder::new();
                let mut new_exprs = Vec::new();
                for &i in &used {
                    new_exprs.push(exprs[i].clone());
                    let f = &row_type.get_field_list()[i];
                    builder.add(f.get_name(), f.get_type().clone());
                }
                RelNode::Project {
                    input: p_input,
                    exprs: new_exprs,
                    row_type: builder.build(),
                }
            } else {
                RelNode::Project {
                    input: p_input,
                    exprs,
                    row_type,
                }
            }
        }
        other => other,
    };
    (r, group_after)
}

fn is_identity(nodes: &[RexNode], row_type: &RelDataType) -> bool {
    let fields = row_type.get_field_list();
    nodes.len() == fields.len()
        && nodes.iter().enumerate().all(|(i, n)| match n {
            RexNode::InputRef { index, ty } => *index == i && ty == fields[i].get_type(),
            _ => false,
        })
}

fn node_count(nodes: &[RexNode]) -> usize {
    nodes.iter().map(RexNode::node_count).sum()
}

fn push_past_project(nodes: &[RexNode], project_exprs: &[RexNode]) -> Vec<RexNode> {
    nodes
        .iter()
        .map(|n| {
            n.shift_refs(&|i| match project_exprs.get(i) {
                Some(e) => e.clone(),
                None => RexNode::InputRef {
                    index: i,
                    ty: RelDataType::basic(SqlTypeName::Any, -1, i32::MIN, true),
                },
            })
        })
        .collect()
}

fn f_suggest(original: Option<&str>, attempt: usize) -> String {
    format!("{}{}", original.unwrap_or("$f"), attempt)
}

pub fn uniquify_names(names: Vec<Option<String>>) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(names.len());
    for (i, name) in names.into_iter().enumerate() {
        let final_name = match name {
            Some(n) if !used.contains(&n) => n,
            other => {
                let mut j = if other.is_none() { i } else { 0 };
                let mut candidate;
                loop {
                    candidate = f_suggest(other.as_deref(), j);
                    j += 1;
                    if !used.contains(&candidate) {
                        break;
                    }
                }
                candidate
            }
        };
        used.insert(final_name.clone());
        out.push(final_name);
    }
    out
}

impl RelBuilder {
    pub fn new(rex_builder: RexBuilder) -> RelBuilder {
        RelBuilder {
            rex_builder,
            simplifier: RexSimplify::new(rex_builder),
            bloat: 100,
        }
    }

    pub fn scan(qualified_name: Vec<String>, row_type: RelDataType) -> RelNode {
        RelNode::TableScan {
            qualified_name,
            row_type,
        }
    }

    pub fn empty(input: &RelNode) -> RelNode {
        RelNode::Values {
            row_type: input.row_type().clone(),
            tuples: Vec::new(),
        }
    }

    pub fn values(row_type: RelDataType, tuples: Vec<Vec<RexLiteral>>) -> RelNode {
        RelNode::Values { row_type, tuples }
    }

    pub fn filter(&self, input: RelNode, predicates: Vec<RexNode>) -> RelNode {
        self.filter_correlated(input, Vec::new(), predicates)
    }

    pub fn filter_correlated(
        &self,
        input: RelNode,
        variables_set: Vec<usize>,
        predicates: Vec<RexNode>,
    ) -> RelNode {
        let simplified = self.simplifier.simplify_filter_predicates(predicates);
        match simplified {
            None => Self::empty(&input),
            Some(c) if c.is_always_false() => Self::empty(&input),
            Some(c) if c.is_always_true() => input,
            Some(c) => RelNode::Filter {
                input: Box::new(input),
                condition: c,
                variables_set,
            },
        }
    }

    pub fn join(
        &self,
        left: RelNode,
        right: RelNode,
        join_type: JoinType,
        condition: &RexNode,
    ) -> RelNode {
        let condition = self.simplifier.simplify_unknown_as_false(condition);
        let row_type = derive_join_row_type(left.row_type(), right.row_type(), join_type);
        RelNode::Join {
            left: Box::new(left),
            right: Box::new(right),
            condition,
            join_type,
            row_type,
        }
    }

    pub fn set_op(kind: SetOpKind, all: bool, inputs: Vec<RelNode>) -> Result<RelNode> {
        let mut inputs = inputs;
        if inputs.len() == 1 {
            return inputs.pop().ok_or_else(|| {
                SubstraitError::illegal_argument("bad INTERSECT/UNION/EXCEPT input count")
            });
        }
        let first = inputs.first().ok_or_else(|| {
            SubstraitError::illegal_argument("bad INTERSECT/UNION/EXCEPT input count")
        })?;
        let mut fields: Vec<(String, RelDataType)> = first
            .row_type()
            .get_field_list()
            .iter()
            .map(|f| (f.get_name().to_string(), f.get_type().clone()))
            .collect();
        for other in inputs.iter().skip(1) {
            let other_fields = other.row_type().get_field_list();
            if other_fields.len() != fields.len() {
                return Err(SubstraitError::illegal_argument(
                    "set op inputs must have the same number of fields",
                ));
            }
            for (f, o) in fields.iter_mut().zip(other_fields) {
                let types = [f.1.clone(), o.get_type().clone()];
                f.1 = crate::isthmus::least_restrictive(&types);
            }
        }
        let mut builder = RelDataTypeBuilder::new();
        for (n, t) in fields {
            builder.add(n, t);
        }
        Ok(RelNode::SetOp {
            kind,
            all,
            inputs,
            row_type: builder.build(),
        })
    }

    pub fn project(&self, input: RelNode, nodes: &[RexNode]) -> RelNode {
        self.project_(input, nodes, Vec::new(), false)
    }

    pub fn project_named(
        &self,
        input: RelNode,
        nodes: &[RexNode],
        names: Vec<Option<String>>,
        force: bool,
    ) -> RelNode {
        self.project_(input, nodes, names, force)
    }

    fn infer_alias(input_row_type: &RelDataType, expr: &RexNode) -> Option<String> {
        match expr {
            RexNode::InputRef { index, .. } => input_row_type
                .get_field_list()
                .get(*index)
                .map(|f| f.get_name().to_string()),
            RexNode::Call(c) if c.op.kind == super::operators::SqlKind::Cast => {
                Self::infer_alias(input_row_type, &c.operands[0])
            }
            _ => None,
        }
    }

    fn project_(
        &self,
        input: RelNode,
        nodes: &[RexNode],
        field_names: Vec<Option<String>>,
        force: bool,
    ) -> RelNode {
        let input_row_type = input.row_type().clone();
        if !force && field_names.is_empty() && is_identity(nodes, &input_row_type) {
            return input;
        }
        let mut names = field_names;
        while names.len() < nodes.len() {
            names.push(None);
        }
        if let RelNode::Project {
            input: bottom_input,
            exprs: bottom_exprs,
            row_type: bottom_row_type,
        } = &input
        {
            for (i, name) in names.iter_mut().enumerate() {
                if name.is_none() {
                    if let RexNode::InputRef { index, .. } = &nodes[i] {
                        *name = bottom_row_type
                            .get_field_list()
                            .get(*index)
                            .map(|f| f.get_name().to_string());
                    }
                }
            }
            let over_conflict = nodes.iter().any(RexNode::contains_over)
                && bottom_exprs.iter().any(RexNode::contains_over);
            if !over_conflict {
                let merged = push_past_project(nodes, bottom_exprs);
                if node_count(&merged) <= node_count(bottom_exprs) + node_count(nodes) + self.bloat
                {
                    let bottom = (**bottom_input).clone();
                    return self.project_(bottom, &merged, names, force);
                }
            }
        }
        let nodes: Vec<RexNode> = nodes
            .iter()
            .map(|e| self.simplifier.simplify_preserving_type(e))
            .collect();
        for (i, name) in names.iter_mut().enumerate() {
            if name.is_none() {
                *name = Self::infer_alias(&input_row_type, &nodes[i]);
            }
        }
        let final_names = uniquify_names(names);
        if !force && is_identity(&nodes, &input_row_type) {
            return input;
        }
        let fixed_rows = match &input {
            RelNode::Values { tuples, .. } => Some(tuples.len()),
            RelNode::Aggregate { group_set, .. } if group_set.is_empty() => Some(1),
            _ => None,
        };
        if let Some(rows) = fixed_rows {
            if nodes.iter().all(|n| matches!(n, RexNode::Literal(_))) {
                let mut builder = RelDataTypeBuilder::new();
                let mut tuple = Vec::new();
                for (name, n) in final_names.iter().zip(nodes.iter()) {
                    builder.add(name.clone(), n.get_type().clone());
                    if let RexNode::Literal(l) = n {
                        tuple.push(l.clone());
                    }
                }
                return RelNode::Values {
                    row_type: builder.build(),
                    tuples: vec![tuple; rows],
                };
            }
        }
        let mut builder = RelDataTypeBuilder::new();
        for (name, n) in final_names.iter().zip(nodes.iter()) {
            builder.add(name.clone(), n.get_type().clone());
        }
        RelNode::Project {
            input: Box::new(input),
            exprs: nodes,
            row_type: builder.build(),
        }
    }

    fn fields(input: &RelNode) -> Vec<RexNode> {
        input
            .row_type()
            .get_field_list()
            .iter()
            .enumerate()
            .map(|(i, f)| RexBuilder::make_input_ref(i, f.get_type().clone()))
            .collect()
    }

    pub fn aggregate(
        &self,
        input: RelNode,
        group_exprs: &[RexNode],
        group_expr_lists: Option<&[Vec<RexNode>]>,
        agg_calls: Vec<AggregateCall>,
    ) -> Result<RelNode> {
        if group_exprs.is_empty() && agg_calls.is_empty() {
            let mut b = RelDataTypeBuilder::new();
            b.add("dummy", self.rex_builder.boolean_type(false));
            let lit = RexLiteral {
                value: LiteralValue::Boolean(true),
                ty: self.rex_builder.boolean_type(false),
            };
            return Ok(RelNode::Values {
                row_type: b.build(),
                tuples: vec![vec![lit]],
            });
        }
        let original = Self::fields(&input);
        let mut extra = original.clone();
        let mut group_list: Vec<usize> = Vec::new();
        for g in group_exprs {
            let idx = if let Some(i) = extra.iter().position(|e| e == g) {
                i
            } else {
                extra.push(g.clone());
                extra.len() - 1
            };
            group_list.push(idx);
        }
        let mut group_set: Vec<usize> = group_list.clone();
        group_set.sort_unstable();
        group_set.dedup();
        let mut group_sets = resolve_group_sets(&extra, &group_set, group_expr_lists)?;
        let r = self.project(input, &extra);
        let mut calls = agg_calls;
        let (r, group_after) = prune_project(r, &group_set, &mut group_sets, &mut calls);
        let mut distinct_calls: Vec<AggregateCall> = Vec::new();
        let mut positions: Vec<(usize, Option<String>)> = Vec::new();
        for c in &calls {
            if let Some(i) = distinct_calls.iter().position(|d| d == c) {
                positions.push((i, c.name.clone()));
            } else {
                distinct_calls.push(c.clone());
                positions.push((distinct_calls.len() - 1, c.name.clone()));
            }
        }
        if distinct_calls.len() == calls.len() {
            let row_type =
                derive_aggregate_row_type(r.row_type(), &group_after, &group_sets, &calls);
            return Ok(RelNode::Aggregate {
                input: Box::new(r),
                group_set: group_after,
                group_sets,
                agg_calls: calls,
                row_type,
            });
        }
        let row_type =
            derive_aggregate_row_type(r.row_type(), &group_after, &group_sets, &distinct_calls);
        let group_count = group_after.len();
        let agg = RelNode::Aggregate {
            input: Box::new(r),
            group_set: group_after,
            group_sets,
            agg_calls: distinct_calls,
            row_type,
        };
        let agg_fields = Self::fields(&agg);
        let mut nodes = Vec::new();
        let mut names = Vec::new();
        for field in agg_fields.iter().take(group_count) {
            nodes.push(field.clone());
            names.push(None);
        }
        for (i, name) in positions {
            nodes.push(agg_fields[group_count + i].clone());
            names.push(name);
        }
        Ok(self.project_(agg, &nodes, names, false))
    }

    pub fn sort_limit(
        &self,
        input: RelNode,
        offset: Option<i64>,
        fetch: Option<i64>,
        keys: Vec<SortKey>,
    ) -> RelNode {
        let offset_node = offset
            .filter(|o| *o > 0)
            .map(|o| self.rex_builder.make_exact_int(o));
        let fetch_node = fetch
            .filter(|f| *f >= 0)
            .map(|f| self.rex_builder.make_exact_int(f));
        let original = Self::fields(&input);
        let mut extra = original.clone();
        let mut collations: Vec<RelFieldCollation> = Vec::new();
        for k in keys {
            let direction = k.direction;
            let null_direction = k
                .null_direction
                .unwrap_or_else(|| direction.default_null_direction());
            let index = match &k.expr {
                RexNode::InputRef { index, .. } => *index,
                other => {
                    extra.push(other.clone());
                    extra.len() - 1
                }
            };
            if !collations.iter().any(|c| c.field_index == index) {
                collations.push(RelFieldCollation::new(index, direction, null_direction));
            }
        }
        let added = extra.len() - original.len();
        if offset_node.is_none() && fetch == Some(0) {
            return Self::empty(&input);
        }
        if offset_node.is_none() && fetch_node.is_none() && collations.is_empty() {
            return input;
        }
        if collations.is_empty() {
            let mergeable = matches!(&input, RelNode::Sort { offset: o2, fetch: f2, .. }
                if (offset_node.is_none() || o2.is_none()) && (fetch_node.is_none() || f2.is_none()));
            if mergeable {
                if let RelNode::Sort {
                    input: i2,
                    collation: c2,
                    offset: o2,
                    fetch: f2,
                } = input
                {
                    return RelNode::Sort {
                        input: i2,
                        collation: c2,
                        offset: offset_node.or(o2),
                        fetch: fetch_node.or(f2),
                    };
                }
            }
            if let RelNode::Project {
                input: p_in,
                exprs,
                row_type,
            } = &input
            {
                if let RelNode::Sort {
                    input: s_in,
                    collation,
                    offset: None,
                    fetch: None,
                } = p_in.as_ref()
                {
                    let sort = RelNode::Sort {
                        input: s_in.clone(),
                        collation: collation.clone(),
                        offset: offset_node,
                        fetch: fetch_node,
                    };
                    return RelNode::Project {
                        input: Box::new(sort),
                        exprs: exprs.clone(),
                        row_type: row_type.clone(),
                    };
                }
            }
        }
        let base = if added > 0 {
            self.project(input, &extra)
        } else {
            input
        };
        let sort = RelNode::Sort {
            input: Box::new(base),
            collation: collations,
            offset: offset_node,
            fetch: fetch_node,
        };
        if added > 0 {
            self.project(sort, &original)
        } else {
            sort
        }
    }
}
