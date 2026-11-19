use std::collections::HashSet;

use bigdecimal::BigDecimal;

use super::operators::{self, SqlKind, SqlOperator};
use super::rel::{AggregateCall, JoinType, RelFieldCollation, RelNode, SetOpKind};
use super::rel_data_type::{RelDataType, RelDataTypeBuilder};
use super::rex::{
    Direction, LiteralValue, NullDirection, RexCall, RexFieldCollation, RexLiteral, RexNode,
    RexOver, RexSubQuery, RexWindowBound, SubQueryKind, Symbol,
};
use super::sarg::{Cut, Range, RangeSet, RexUnknownAs, Sarg, SargValue};
use super::sql_dialect::SqlDialect;
use super::sql_node::{
    contains_agg_list, is_singleton_star, SqlCall, SqlCase, SqlIdentifier, SqlLiteral,
    SqlLiteralValue, SqlNode, SqlSelect, SqlWindow,
};
use super::sql_type_name::{SqlTypeFamily, SqlTypeName};
use super::values::{java_big_decimal_to_plain_string, java_double_to_string};
use crate::error::{Result, SubstraitError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clause {
    From,
    Where,
    GroupBy,
    Having,
    Select,
    SetOp,
    OrderBy,
    Fetch,
    Offset,
}

impl Clause {
    const ALL: [Clause; 9] = [
        Clause::From,
        Clause::Where,
        Clause::GroupBy,
        Clause::Having,
        Clause::Select,
        Clause::SetOp,
        Clause::OrderBy,
        Clause::Fetch,
        Clause::Offset,
    ];

    fn bit(self) -> u16 {
        1 << self as u16
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Clauses(u16);

impl Clauses {
    fn of(list: &[Clause]) -> Clauses {
        Clauses(list.iter().fold(0, |bits, c| bits | c.bit()))
    }

    fn contains(self, clause: Clause) -> bool {
        self.0 & clause.bit() != 0
    }

    fn union(self, other: Clauses) -> Clauses {
        Clauses(self.0 | other.0)
    }

    fn iter(self) -> impl Iterator<Item = Clause> {
        Clause::ALL.into_iter().filter(move |c| self.contains(*c))
    }

    fn max(self) -> Option<Clause> {
        self.iter().last()
    }
}

impl From<Clause> for Clauses {
    fn from(clause: Clause) -> Clauses {
        Clauses(clause.bit())
    }
}

#[derive(Debug, Clone)]
struct Frame {
    anon: bool,
    ignore_clauses: bool,
    expected_clauses: Clauses,
}

type Aliases = Vec<(String, RelDataType)>;

#[derive(Debug, Clone)]
pub struct SqlResult {
    node: SqlNode,
    clauses: Clauses,
    needed_alias: Option<String>,
    needed_type: Option<RelDataType>,
    aliases: Aliases,
    anon: bool,
    ignore_clauses: bool,
    expected_clauses: Clauses,
}

#[derive(Debug, Clone)]
enum ContextKind {
    Alias { aliases: Aliases, qualified: bool },
    SelectList { items: Vec<SqlNode> },
}

#[derive(Debug, Clone)]
pub struct Context {
    kind: ContextKind,
    dialect: SqlDialect,
    correl: Vec<(usize, Aliases)>,
    subqueries: Vec<(RexSubQuery, SqlNode)>,
}

pub struct Builder {
    clauses: Clauses,
    select: SqlSelect,
    context: Context,
    anon: bool,
    aliases: Option<Aliases>,
    need_new: bool,
}

fn uniquify_expr(name: &str, used: &mut HashSet<String>) -> String {
    if used.insert(name.to_string()) {
        return name.to_string();
    }
    let mut j = 0;
    loop {
        let candidate = format!("{name}{j}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        j += 1;
    }
}

fn adjusted_row_type(row_type: &RelDataType, node: &SqlNode) -> RelDataType {
    match node {
        SqlNode::Select(s) => {
            if is_singleton_star(&s.select_list) {
                return row_type.clone();
            }
            let mut b = RelDataTypeBuilder::new();
            for (item, field) in s.select_list.iter().zip(row_type.field_list().iter()) {
                let name = item.alias().unwrap_or_else(|| field.name().to_string());
                b.add(name, field.ty().clone());
            }
            b.build()
        }
        SqlNode::Call(c) if c.op.kind == SqlKind::As && c.operands.len() > 2 => {
            let mut b = RelDataTypeBuilder::new();
            for (op, field) in c.operands[2..].iter().zip(row_type.field_list().iter()) {
                b.add(op.to_plain_string(), field.ty().clone());
            }
            b.build()
        }
        _ => row_type.clone(),
    }
}

fn is_star(
    exprs: &[RexNode],
    input_row_type: &RelDataType,
    project_row_type: &RelDataType,
) -> bool {
    for (i, e) in exprs.iter().enumerate() {
        match e {
            RexNode::InputRef { index, .. } if *index == i => {}
            _ => return false,
        }
    }
    exprs.len() == input_row_type.field_count()
        && input_row_type.field_names() == project_row_type.field_names()
}

fn add_select(list: &mut Vec<SqlNode>, node: SqlNode, row_type: &RelDataType) {
    let names = row_type.field_names();
    let name = names.get(list.len()).cloned().unwrap_or_default();
    let alias = node.alias();
    let node = match alias {
        Some(a) if a == name => node,
        _ => SqlNode::as_alias(node, name),
    };
    list.push(node);
}

fn create_call(op: &'static SqlOperator, operands: Vec<SqlNode>) -> SqlNode {
    match op.kind {
        SqlKind::And | SqlKind::Or => match operands.len() {
            0 => return SqlNode::Literal(SqlLiteral::boolean(op.kind == SqlKind::And)),
            1 => {
                return operands
                    .into_iter()
                    .next()
                    .unwrap_or(SqlNode::Literal(SqlLiteral::boolean(true)))
            }
            2..=5 => {}
            n => return create_balanced_call(op, &operands, 0, n),
        },
        _ => {}
    }
    if op.syntax == operators::SqlSyntax::Binary && operands.len() > 2 {
        let mut it = operands.into_iter();
        let a = it.next();
        let b = it.next();
        let mut node = SqlNode::call(op, a.into_iter().chain(b).collect());
        for rest in it {
            node = SqlNode::call(op, vec![node, rest]);
        }
        return node;
    }
    SqlNode::call(op, operands)
}

fn create_balanced_call(
    op: &'static SqlOperator,
    operands: &[SqlNode],
    start: usize,
    end: usize,
) -> SqlNode {
    if start + 1 == end {
        return operands[start].clone();
    }
    let mid = (end - start) / 2 + start;
    let left = create_balanced_call(op, operands, start, mid);
    let right = create_balanced_call(op, operands, mid, end);
    SqlNode::call(op, vec![left, right])
}

fn and_expressions(node1: Option<SqlNode>, node2: SqlNode) -> SqlNode {
    match node1 {
        None => node2,
        Some(n1) => {
            let mut list = Vec::new();
            match n1 {
                SqlNode::Call(c) if c.op.kind == SqlKind::And => list.extend(c.operands),
                other => list.push(other),
            }
            match node2 {
                SqlNode::Call(c) if c.op.kind == SqlKind::And => list.extend(c.operands),
                other => list.push(other),
            }
            SqlNode::call(&operators::AND, list)
        }
    }
}

fn has_sort_by_ordinal(node: &SqlNode) -> bool {
    match node {
        SqlNode::Literal(SqlLiteral {
            value: SqlLiteralValue::Exact(_) | SqlLiteralValue::Approx(_),
        }) => true,
        SqlNode::Select(s) => match &s.order_by {
            None => false,
            Some(list) => list.iter().any(has_sort_by_ordinal),
        },
        SqlNode::Call(c) => c.operands.iter().any(has_sort_by_ordinal),
        _ => false,
    }
}

fn contains_over_node(node: &SqlNode) -> bool {
    match node {
        SqlNode::Window(_) => true,
        SqlNode::Select(s) => s.select_list.iter().any(contains_over_node),
        SqlNode::Call(c) => c.operands.iter().any(contains_over_node),
        _ => false,
    }
}

fn visit_calls(node: &SqlNode, pred: &impl Fn(&SqlCall) -> bool) -> bool {
    match node {
        SqlNode::Call(c) => pred(c) || c.operands.iter().any(|o| visit_calls(o, pred)),
        SqlNode::Case(c) => {
            c.value.as_ref().is_some_and(|v| visit_calls(v, pred))
                || c.when_list.iter().any(|o| visit_calls(o, pred))
                || c.then_list.iter().any(|o| visit_calls(o, pred))
                || c.else_expr.as_ref().is_some_and(|v| visit_calls(v, pred))
        }
        SqlNode::NodeList(l) => l.iter().any(|o| visit_calls(o, pred)),
        SqlNode::Window(w) => w
            .partition_list
            .iter()
            .chain(w.order_list.iter())
            .any(|o| visit_calls(o, pred)),
        _ => false,
    }
}

fn literal_from_sarg_value(v: &SargValue, ty: &RelDataType) -> RexLiteral {
    RexLiteral {
        value: LiteralValue::from_sarg_value(v),
        ty: ty.clone(),
    }
}

impl SqlResult {
    fn with_anon(mut self, anon: bool) -> SqlResult {
        self.anon = anon;
        self
    }

    fn reset_alias(self) -> SqlResult {
        match (&self.needed_alias, &self.needed_type) {
            (Some(a), Some(t)) => {
                let aliases = vec![(a.clone(), t.clone())];
                SqlResult { aliases, ..self }
            }
            _ => self,
        }
    }

    fn to_from(&self) -> SqlNode {
        match &self.needed_alias {
            Some(alias) => {
                let id = SqlNode::identifier(alias.clone());
                match &self.node {
                    SqlNode::Call(c) if c.op.kind == SqlKind::As => {
                        let mut ops = c.operands.clone();
                        if ops.len() > 1 {
                            ops[1] = id;
                        }
                        SqlNode::call(&operators::AS, ops)
                    }
                    other => SqlNode::call(&operators::AS, vec![other.clone(), id]),
                }
            }
            None => self.node.clone(),
        }
    }

    fn sub_select(&self, dialect: &SqlDialect) -> SqlSelect {
        wrap_select(dialect, self.to_from())
    }

    fn to_select(&self, dialect: &SqlDialect) -> SqlSelect {
        match &self.node {
            SqlNode::Select(s) => (**s).clone(),
            other => wrap_select(dialect, other.clone()),
        }
    }

    pub fn to_statement(&self, dialect: &SqlDialect) -> SqlNode {
        if matches!(
            self.node.kind(),
            SqlKind::Union | SqlKind::Intersect | SqlKind::Except
        ) {
            return self.node.clone();
        }
        let node = SqlNode::Select(Box::new(self.to_select(dialect)));
        if self.anon {
            strip_trivial_aliases(node)
        } else {
            node
        }
    }

    fn need_new_sub_query(&self, rel: &RelNode, dialect: &SqlDialect, expected: Clauses) -> bool {
        let clauses = self.clauses;
        let Some(max_clause) = clauses.max() else {
            return false;
        };
        for c in expected.iter() {
            if max_clause > c || (max_clause == c && c != Clause::Select) {
                return true;
            }
        }
        let node = &self.node;
        let contains_over = contains_over_node(node);
        if rel.project_contains_over() && contains_over {
            return true;
        }
        if let RelNode::Project { exprs, input, .. } = rel {
            if clauses.contains(Clause::OrderBy) && has_sort_by_ordinal(node) {
                return true;
            }
            if let RelNode::Aggregate {
                group_set,
                agg_calls,
                ..
            } = input.as_ref()
            {
                let mut used = Vec::new();
                for e in exprs {
                    e.collect_input_refs(&mut used);
                }
                let has_aggregate = !group_set.is_empty() || !agg_calls.is_empty();
                if has_aggregate && used.is_empty() {
                    return true;
                }
            }
        }
        if let RelNode::Aggregate {
            group_set,
            agg_calls,
            ..
        } = rel
        {
            if clauses.contains(Clause::OrderBy)
                || clauses.contains(Clause::Fetch)
                || clauses.contains(Clause::Offset)
            {
                return true;
            }
            let has_nested_agg = self.has_nested(agg_calls, &|c: &SqlCall| c.op.aggregate);
            let has_nested_windowed =
                self.has_nested(agg_calls, &|c: &SqlCall| c.op.kind == SqlKind::Over);
            if !dialect.supports_nested_aggregations() && (has_nested_agg || has_nested_windowed) {
                return true;
            }
            if clauses.contains(Clause::GroupBy) {
                return !has_nested_agg || !group_set.is_empty();
            }
        }
        false
    }

    fn has_nested(&self, agg_calls: &[AggregateCall], pred: &impl Fn(&SqlCall) -> bool) -> bool {
        let SqlNode::Select(s) = &self.node else {
            return false;
        };
        if is_singleton_star(&s.select_list) {
            return false;
        }
        let mut args: Vec<usize> = Vec::new();
        for c in agg_calls {
            for a in &c.args {
                if !args.contains(a) {
                    args.push(*a);
                }
            }
        }
        args.iter().any(|a| match s.select_list.get(*a) {
            Some(item @ SqlNode::Call(_)) => visit_calls(item, pred),
            _ => false,
        })
    }
}

fn strip_trivial_aliases(node: SqlNode) -> SqlNode {
    match node {
        SqlNode::Select(mut s) => {
            for item in &mut s.select_list {
                if let SqlNode::Call(c) = item {
                    if c.op.kind == SqlKind::As {
                        let generated = c
                            .operands
                            .get(1)
                            .and_then(SqlNode::as_identifier)
                            .and_then(SqlIdentifier::as_simple)
                            .is_some_and(|n| n.starts_with("EXPR$"));
                        if generated {
                            *item = c.operands[0].clone();
                        }
                    }
                }
            }
            SqlNode::Select(s)
        }
        other => other,
    }
}

fn requires_alias(dialect: &SqlDialect, node: &SqlNode) -> bool {
    dialect.requires_alias_for_from_items()
        && !matches!(
            node.kind(),
            SqlKind::Identifier | SqlKind::As | SqlKind::Join
        )
}

fn wrap_select(dialect: &SqlDialect, node: SqlNode) -> SqlSelect {
    let node = if requires_alias(dialect, &node) {
        SqlNode::as_alias(node, "t")
    } else {
        node
    };
    SqlSelect {
        select_list: vec![SqlNode::star()],
        from: Some(node),
        ..SqlSelect::default()
    }
}

impl Builder {
    fn add_order_item(&self, list: &mut Vec<SqlNode>, field: &RelFieldCollation) -> Result<()> {
        list.push(self.context.collation_to_sql(field)?);
        Ok(())
    }
}

impl Context {
    fn alias_context(dialect: &SqlDialect, aliases: Aliases, qualified: bool) -> Context {
        Context {
            kind: ContextKind::Alias { aliases, qualified },
            dialect: *dialect,
            correl: Vec::new(),
            subqueries: Vec::new(),
        }
    }

    fn correl_field(&self, correl: usize, index: usize) -> Result<SqlNode> {
        let aliases = self
            .correl
            .iter()
            .rev()
            .find(|(id, _)| *id == correl)
            .map(|(_, a)| a.clone())
            .ok_or_else(|| {
                SubstraitError::IllegalState(format!("unknown correlation variable $cor{correl}"))
            })?;
        Context::alias_context(&self.dialect, aliases, true).field(index)
    }

    fn sub_query_to_sql(&self, q: &RexSubQuery) -> Result<SqlNode> {
        let rel = self
            .subqueries
            .iter()
            .find(|(k, _)| k == q)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                SubstraitError::IllegalState(
                    "sub-query was not converted before its expression".into(),
                )
            })?;
        match q.kind {
            SubQueryKind::In => {
                let op0 = if q.operands.len() == 1 {
                    self.to_sql(&q.operands[0])?
                } else {
                    SqlNode::NodeList(self.to_sql_list(&q.operands)?)
                };
                Ok(SqlNode::call(&operators::IN, vec![op0, rel]))
            }
            SubQueryKind::Exists => Ok(SqlNode::call(&operators::EXISTS, vec![rel])),
            SubQueryKind::Unique => Ok(SqlNode::call(&operators::UNIQUE, vec![rel])),
            SubQueryKind::Scalar => Ok(SqlNode::call(&operators::SCALAR_QUERY, vec![rel])),
        }
    }

    pub fn field(&self, ordinal: usize) -> Result<SqlNode> {
        match &self.kind {
            ContextKind::Alias { aliases, qualified } => {
                let mut ord = ordinal;
                for (alias, ty) in aliases {
                    let fields = ty.field_list();
                    if ord < fields.len() {
                        let name = fields[ord].name().to_string();
                        let names = if *qualified {
                            vec![alias.clone(), name]
                        } else {
                            vec![name]
                        };
                        return Ok(SqlNode::Identifier(SqlIdentifier::new(names)));
                    }
                    ord -= fields.len();
                }
                Err(SubstraitError::IllegalState(format!(
                    "field ordinal {ordinal} out of range"
                )))
            }
            ContextKind::SelectList { items } => {
                let item = items.get(ordinal).ok_or_else(|| {
                    SubstraitError::IndexOutOfBounds(format!("Index {ordinal} out of bounds"))
                })?;
                match item {
                    SqlNode::Call(c) if c.op.kind == SqlKind::As => Ok(c.operands[0].clone()),
                    other => Ok(other.clone()),
                }
            }
        }
    }

    fn base_order_field(&self, ordinal: usize) -> Result<SqlNode> {
        let node = self.field(ordinal)?;
        match &node {
            SqlNode::Literal(l)
                if matches!(
                    l.value,
                    SqlLiteralValue::Exact(_) | SqlLiteralValue::Approx(_)
                ) =>
            {
                Ok(SqlNode::Literal(SqlLiteral::char_string(l.to_value())))
            }
            SqlNode::Call(_) | SqlNode::Case(_) => Ok(SqlNode::Literal(SqlLiteral::exact(
                BigDecimal::from(ordinal_number(ordinal)?),
            ))),
            _ => Ok(node),
        }
    }

    pub fn order_field(&self, ordinal: usize) -> Result<SqlNode> {
        let node = self.base_order_field(ordinal)?;
        if let ContextKind::SelectList { items, .. } = &self.kind {
            if let SqlNode::Identifier(id) = &node {
                if let Some(name) = id.as_simple() {
                    for (i, item) in items.iter().enumerate() {
                        if i != ordinal {
                            if let Some(alias) = item.alias() {
                                if name.eq_ignore_ascii_case(&alias)
                                    && self.dialect.conformance().is_sort_by_alias()
                                {
                                    return Ok(SqlNode::Literal(SqlLiteral::exact(
                                        BigDecimal::from(ordinal_number(ordinal)?),
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(node)
    }

    pub fn collation_to_sql(&self, collation: &RelFieldCollation) -> Result<SqlNode> {
        let mut node = self.order_field(collation.field_index)?;
        if collation.direction == Direction::Descending {
            node = SqlNode::call(&operators::DESC, vec![node]);
        }
        if collation.null_direction != collation.direction.default_null_direction() {
            match collation.null_direction {
                NullDirection::First => node = SqlNode::call(&operators::NULLS_FIRST, vec![node]),
                NullDirection::Last => node = SqlNode::call(&operators::NULLS_LAST, vec![node]),
                NullDirection::Unspecified => {}
            }
        }
        Ok(node)
    }

    fn to_sql_list(&self, list: &[RexNode]) -> Result<Vec<SqlNode>> {
        list.iter().map(|r| self.to_sql(r)).collect()
    }

    pub fn to_sql(&self, rex: &RexNode) -> Result<SqlNode> {
        match rex {
            RexNode::InputRef { index, .. } => self.field(*index),
            RexNode::Literal(l) => literal_to_sql(l),
            RexNode::Over(o) => self.over_to_sql(o),
            RexNode::SubQuery(q) => self.sub_query_to_sql(q),
            RexNode::CorrelField { correl, index, .. } => self.correl_field(*correl, *index),
            RexNode::Call(call) => match call.op.kind {
                SqlKind::Case => self.case_to_sql(call),
                SqlKind::Search => match call.operands.get(1) {
                    Some(RexNode::Literal(RexLiteral {
                        value: LiteralValue::Sarg(sarg),
                        ty,
                    })) => self.sarg_to_sql(&call.operands[0], ty, sarg),
                    _ => Err(SubstraitError::IllegalState(
                        "SEARCH without a Sarg literal".into(),
                    )),
                },
                SqlKind::Not => {
                    let operand = &call.operands[0];
                    let node = self.to_sql(operand)?;
                    let inverse = match operand {
                        RexNode::SubQuery(q) if q.kind == SubQueryKind::In => {
                            Some(&operators::NOT_IN)
                        }
                        _ => operand.as_call().and_then(|c| c.op.not()),
                    };
                    match (inverse, node) {
                        (Some(inv), SqlNode::Call(c)) => Ok(SqlNode::call(inv, c.operands)),
                        (_, node) => Ok(SqlNode::call(&operators::NOT, vec![node])),
                    }
                }
                _ => self.call_to_sql(call, false),
            },
        }
    }

    fn case_to_sql(&self, call: &RexCall) -> Result<SqlNode> {
        let nodes = self.to_sql_list(&call.operands)?;
        let n = nodes.len();
        let mut when_list = Vec::new();
        let mut then_list = Vec::new();
        let value;
        if n % 2 == 0 {
            value = nodes.first().cloned().map(Box::new);
            let mut i = 1;
            while i + 1 < n {
                when_list.push(nodes[i].clone());
                then_list.push(nodes[i + 1].clone());
                i += 2;
            }
        } else {
            value = None;
            let mut i = 0;
            while i + 1 < n {
                when_list.push(nodes[i].clone());
                then_list.push(nodes[i + 1].clone());
                i += 2;
            }
        }
        let else_expr = nodes.last().cloned().map(Box::new);
        Ok(SqlNode::Case(Box::new(SqlCase {
            value,
            when_list,
            then_list,
            else_expr,
        })))
    }

    fn strip_cast_from_string(&self, call: &RexCall) -> RexCall {
        match call.op.kind {
            SqlKind::Equals
            | SqlKind::IsNotDistinctFrom
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThan
            | SqlKind::LessThanOrEqual => {
                let o0 = &call.operands[0];
                let o1 = &call.operands[1];
                let is_cast = |n: &RexNode| n.kind() == SqlKind::Cast;
                let coercion = |n: &RexNode| -> bool {
                    match n {
                        RexNode::Call(c) => {
                            let inner = &c.operands[0];
                            self.dialect.supports_implicit_type_coercion(
                                &c.ty,
                                inner.ty(),
                                matches!(inner, RexNode::Literal(_)),
                            )
                        }
                        _ => false,
                    }
                };
                if is_cast(o0) && !is_cast(o1) {
                    if !coercion(o0) {
                        return call.clone();
                    }
                    let inner = o0.operands()[0].clone();
                    return RexCall {
                        op: call.op,
                        operands: vec![inner, o1.clone()],
                        ty: call.ty.clone(),
                    };
                }
                if is_cast(o1) && !is_cast(o0) {
                    if !coercion(o1) {
                        return call.clone();
                    }
                    let inner = o1.operands()[0].clone();
                    return RexCall {
                        op: call.op,
                        operands: vec![o0.clone(), inner],
                        ty: call.ty.clone(),
                    };
                }
                call.clone()
            }
            _ => call.clone(),
        }
    }

    fn call_to_sql(&self, call0: &RexCall, not: bool) -> Result<SqlNode> {
        let call = self.strip_cast_from_string(call0);
        let mut op: &'static SqlOperator = call.op;
        match op.kind {
            SqlKind::Sum0 => op = &operators::SUM,
            SqlKind::Not => {
                if let Some(RexNode::Call(inner)) = call.operands.first() {
                    if inner.op.not().is_some() {
                        return self.call_to_sql(inner, !not);
                    }
                }
            }
            _ => {}
        }
        if not {
            op = op.not().ok_or_else(|| {
                SubstraitError::IllegalState(format!("unable to negate {:?}", call.op.kind))
            })?;
        }
        let mut nodes = self.to_sql_list(&call.operands)?;
        if matches!(call.op.kind, SqlKind::Cast | SqlKind::SafeCast) {
            if let Some(spec) = self.dialect.cast_spec(&call.ty) {
                nodes.push(SqlNode::DataType(spec));
            }
        }
        Ok(create_call(op, nodes))
    }

    fn sarg_to_sql(&self, operand: &RexNode, ty: &RelDataType, sarg: &Sarg) -> Result<SqlNode> {
        let mut or_list = Vec::new();
        let operand_sql = self.to_sql(operand)?;
        if sarg.null_as == RexUnknownAs::True {
            or_list.push(SqlNode::call(
                &operators::IS_NULL,
                vec![operand_sql.clone()],
            ));
        }
        if sarg.is_points() {
            or_list.push(Self::to_in(
                &operand_sql,
                &operators::EQUALS,
                &operators::IN,
                ty,
                &sarg.range_set,
            )?);
        } else if sarg.is_complemented_points() {
            let complement = sarg.range_set.complement();
            or_list.push(Self::to_in(
                &operand_sql,
                &operators::NOT_EQUALS,
                &operators::NOT_IN,
                ty,
                &complement,
            )?);
        } else {
            for r in sarg.range_set.as_ranges() {
                or_list.push(Self::range_to_sql(&operand_sql, ty, r)?);
            }
        }
        Ok(create_call(&operators::OR, or_list))
    }

    fn value_sql(v: &SargValue, ty: &RelDataType) -> Result<SqlNode> {
        literal_to_sql(&literal_from_sarg_value(v, ty))
    }

    fn range_to_sql(arg: &SqlNode, ty: &RelDataType, r: &Range) -> Result<SqlNode> {
        let op = |o: &'static SqlOperator, v: &SargValue| -> Result<SqlNode> {
            Ok(SqlNode::call(o, vec![arg.clone(), Self::value_sql(v, ty)?]))
        };
        let and = |a: SqlNode, b: SqlNode| create_call(&operators::AND, vec![a, b]);
        match (&r.lower, &r.upper) {
            (Cut::BelowAll, Cut::AboveAll) => Ok(SqlNode::Literal(SqlLiteral::boolean(true))),
            (Cut::BelowAll, Cut::Above(u)) => op(&operators::LESS_THAN_OR_EQUAL, u),
            (Cut::BelowAll, Cut::Below(u)) => op(&operators::LESS_THAN, u),
            (Cut::Below(l), Cut::AboveAll) => op(&operators::GREATER_THAN_OR_EQUAL, l),
            (Cut::Above(l), Cut::AboveAll) => op(&operators::GREATER_THAN, l),
            (Cut::Above(l), Cut::Below(u)) => Ok(and(
                op(&operators::GREATER_THAN, l)?,
                op(&operators::LESS_THAN, u)?,
            )),
            (Cut::Above(l), Cut::Above(u)) => Ok(and(
                op(&operators::GREATER_THAN, l)?,
                op(&operators::LESS_THAN_OR_EQUAL, u)?,
            )),
            (Cut::Below(l), Cut::Below(u)) => Ok(and(
                op(&operators::GREATER_THAN_OR_EQUAL, l)?,
                op(&operators::LESS_THAN, u)?,
            )),
            (Cut::Below(l), Cut::Above(u)) => {
                if l == u {
                    op(&operators::EQUALS, l)
                } else {
                    Ok(and(
                        op(&operators::GREATER_THAN_OR_EQUAL, l)?,
                        op(&operators::LESS_THAN_OR_EQUAL, u)?,
                    ))
                }
            }
            _ => Ok(SqlNode::Literal(SqlLiteral::boolean(false))),
        }
    }

    fn to_in(
        operand_sql: &SqlNode,
        eq_op: &'static SqlOperator,
        in_op: &'static SqlOperator,
        ty: &RelDataType,
        range_set: &RangeSet,
    ) -> Result<SqlNode> {
        let mut list = Vec::new();
        for r in range_set.as_ranges() {
            if let Some(v) = r.lower.endpoint() {
                list.push(literal_to_sql(&literal_from_sarg_value(v, ty))?);
            }
        }
        if list.len() == 1 {
            let only = list.remove(0);
            return Ok(SqlNode::call(eq_op, vec![operand_sql.clone(), only]));
        }
        Ok(SqlNode::call(
            in_op,
            vec![operand_sql.clone(), SqlNode::NodeList(list)],
        ))
    }

    fn rex_collation_to_sql(&self, rfc: &RexFieldCollation) -> Result<SqlNode> {
        let mut node = self.to_sql(&rfc.expr)?;
        if rfc.direction == Direction::Descending {
            node = SqlNode::call(&operators::DESC, vec![node]);
        }
        if rfc.null_direction != rfc.direction.default_null_direction() {
            match rfc.null_direction {
                NullDirection::First => node = SqlNode::call(&operators::NULLS_FIRST, vec![node]),
                NullDirection::Last => node = SqlNode::call(&operators::NULLS_LAST, vec![node]),
                NullDirection::Unspecified => {}
            }
        }
        Ok(node)
    }

    fn window_bound(&self, b: &RexWindowBound) -> Result<SqlNode> {
        Ok(match b {
            RexWindowBound::CurrentRow => SqlNode::Literal(SqlLiteral::symbol("CURRENT ROW")),
            RexWindowBound::UnboundedPreceding => {
                SqlNode::Literal(SqlLiteral::symbol("UNBOUNDED PRECEDING"))
            }
            RexWindowBound::UnboundedFollowing => {
                SqlNode::Literal(SqlLiteral::symbol("UNBOUNDED FOLLOWING"))
            }
            RexWindowBound::Preceding(e) => {
                SqlNode::call(&operators::PRECEDING, vec![self.to_sql(e)?])
            }
            RexWindowBound::Following(e) => {
                SqlNode::call(&operators::FOLLOWING, vec![self.to_sql(e)?])
            }
        })
    }

    fn over_to_sql(&self, over: &RexOver) -> Result<SqlNode> {
        let partition_list = self.to_sql_list(&over.window.partition_keys)?;
        let mut order_list = Vec::new();
        for rfc in &over.window.order_keys {
            order_list.push(self.rex_collation_to_sql(rfc)?);
        }
        let (lower, upper) = if over.op.allows_framing() {
            (
                Some(Box::new(self.window_bound(&over.window.lower_bound)?)),
                Some(Box::new(self.window_bound(&over.window.upper_bound)?)),
            )
        } else {
            (None, None)
        };
        let window = SqlNode::Window(Box::new(SqlWindow {
            partition_list,
            order_list,
            is_rows: over.window.is_rows,
            lower_bound: lower,
            upper_bound: upper,
        }));
        let operands = self.to_sql_list(&over.operands)?;
        Ok(create_over_call(over.op, operands, window, over.distinct))
    }

    pub fn agg_to_sql(&self, call: &AggregateCall) -> Result<SqlNode> {
        let mut operands = Vec::new();
        for a in &call.args {
            operands.push(self.field(*a)?);
        }
        self.agg_to_sql_inner(
            call.op,
            call.distinct,
            operands,
            call.filter_arg,
            &call.collation,
            call.approximate,
        )
    }

    fn agg_to_sql_inner(
        &self,
        op: &'static SqlOperator,
        distinct: bool,
        operands: Vec<SqlNode>,
        filter_arg: Option<usize>,
        collation: &[RelFieldCollation],
        approximate: bool,
    ) -> Result<SqlNode> {
        if op.kind == SqlKind::Sum0 {
            let node = self.agg_to_sql_inner(
                &operators::SUM,
                distinct,
                operands,
                filter_arg,
                collation,
                approximate,
            )?;
            return Ok(SqlNode::call(
                &operators::COALESCE,
                vec![
                    node,
                    SqlNode::Literal(SqlLiteral::exact(BigDecimal::from(0))),
                ],
            ));
        }
        let operands = if op.kind == SqlKind::Count && operands.is_empty() {
            vec![SqlNode::star()]
        } else {
            operands
        };
        let call = SqlNode::Call(SqlCall {
            op,
            operands: operands.clone(),
            distinct,
        });
        let call2 = if distinct && approximate && self.dialect.supports_approx_count_distinct() {
            SqlNode::call(&operators::APPROX_COUNT_DISTINCT, operands)
        } else if let Some(f) = filter_arg {
            SqlNode::call(&FILTER, vec![call, self.field(f)?])
        } else {
            call
        };
        if collation.is_empty() {
            return Ok(call2);
        }
        let mut order = Vec::new();
        for c in collation {
            order.push(self.collation_to_sql(c)?);
        }
        Ok(SqlNode::call(
            &WITHIN_GROUP,
            vec![call2, SqlNode::NodeList(order)],
        ))
    }
}

pub static FILTER: SqlOperator = SqlOperator {
    name: "FILTER",
    kind: SqlKind::Other,
    left_prec: 20,
    right_prec: 21,
    syntax: operators::SqlSyntax::Special,
    aggregate: false,
    negated: false,
};

pub static WITHIN_GROUP: SqlOperator = SqlOperator {
    name: "WITHIN GROUP",
    kind: SqlKind::Other,
    left_prec: 100,
    right_prec: 100,
    syntax: operators::SqlSyntax::Special,
    aggregate: false,
    negated: false,
};

fn create_over_call(
    op: &'static SqlOperator,
    operands: Vec<SqlNode>,
    window: SqlNode,
    distinct: bool,
) -> SqlNode {
    if op.kind == SqlKind::Sum0 {
        let node = create_over_call(&operators::SUM, operands, window, distinct);
        return SqlNode::call(
            &operators::COALESCE,
            vec![
                node,
                SqlNode::Literal(SqlLiteral::exact(BigDecimal::from(0))),
            ],
        );
    }
    let agg = SqlNode::Call(SqlCall {
        op,
        operands,
        distinct,
    });
    SqlNode::call(&operators::OVER, vec![agg, window])
}

fn java_double_decimal(value: f64) -> BigDecimal {
    java_double_to_string(value).parse().unwrap_or_default()
}

fn ordinal_number(ordinal: usize) -> Result<u64> {
    u64::try_from(ordinal + 1)
        .map_err(|_| SubstraitError::IndexOutOfBounds(format!("Index {ordinal} out of bounds")))
}

pub fn literal_to_sql(literal: &RexLiteral) -> Result<SqlNode> {
    let tn = literal.ty.sql_type_name();
    let lit = match &literal.value {
        LiteralValue::Symbol(Symbol::TimeUnit(t)) => SqlLiteral::symbol(t.name()),
        LiteralValue::Symbol(Symbol::Trim(t)) => SqlLiteral::symbol(t.name()),
        LiteralValue::Sarg(_) => {
            return Err(SubstraitError::IllegalState(
                "sargs should be handled as part of predicates, not as literals".into(),
            ))
        }
        LiteralValue::Null => SqlLiteral::null(),
        LiteralValue::Char(s) => SqlLiteral::char_string(s.clone()),
        LiteralValue::Exact(d) => {
            if tn.is_approx() {
                SqlLiteral::approx(java_double_decimal(
                    d.to_string().parse::<f64>().unwrap_or(0.0),
                ))
            } else {
                SqlLiteral::exact(d.clone())
            }
        }
        LiteralValue::Approx(f) => {
            if tn.is_approx() || tn.family() != SqlTypeFamily::Numeric {
                SqlLiteral::approx(java_double_decimal(*f))
            } else {
                SqlLiteral::exact(java_double_decimal(*f))
            }
        }
        LiteralValue::Boolean(b) => SqlLiteral::boolean(*b),
        LiteralValue::Date(d) => SqlLiteral {
            value: SqlLiteralValue::Date(d.clone()),
        },
        LiteralValue::Time(t) => SqlLiteral {
            value: SqlLiteralValue::Time(t.clone(), literal.ty.precision()),
        },
        LiteralValue::Timestamp(t) => SqlLiteral {
            value: SqlLiteralValue::Timestamp(t.clone(), literal.ty.precision()),
        },
        LiteralValue::Binary(b) => SqlLiteral {
            value: SqlLiteralValue::Binary(b.clone()),
        },
        LiteralValue::Interval(d) => {
            let negative = d < &BigDecimal::from(0);
            let abs = if negative { -d.clone() } else { d.clone() };
            SqlLiteral {
                value: SqlLiteralValue::Interval {
                    sign: if negative { -1 } else { 1 },
                    text: interval_string(&abs, tn),
                    qualifier: literal.ty.interval_qualifier(),
                },
            }
        }
    };
    Ok(SqlNode::Literal(lit))
}

fn interval_string(value: &BigDecimal, type_name: SqlTypeName) -> String {
    let units: &[(i64, &str, i32)] = if type_name == SqlTypeName::IntervalYearMonth {
        &[(12, " ", -1), (1, "-", -1)]
    } else {
        &[
            (86_400_000, "-", -1),
            (3_600_000, " ", 2),
            (60_000, ":", 2),
            (1_000, ":", 2),
            (1, ".", 3),
        ]
    };
    let mut v = value.clone();
    let mut b = String::new();
    for (multiplier, separator, width) in units {
        let m = BigDecimal::from(*multiplier);
        let quotient = (&v / &m).with_scale_round(0, bigdecimal::RoundingMode::Down);
        let remainder = &v - &quotient * &m;
        let first = b.is_empty();
        if !first {
            b.push_str(separator);
        }
        let digits = java_big_decimal_to_plain_string(&quotient);
        if !first {
            for _ in digits.chars().count()..usize::try_from(*width).unwrap_or(0) {
                b.push('0');
            }
        }
        b.push_str(&digits);
        v = remainder;
    }
    if type_name == SqlTypeName::IntervalDaySecond {
        while let Some(dot) = b.rfind('.') {
            if !b.ends_with('0') || !b[dot + 1..].chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            if b.ends_with(".0") {
                b.truncate(b.len() - 2);
            } else {
                b.truncate(b.len() - 1);
            }
        }
    }
    b
}

pub struct RelToSqlConverter {
    dialect: SqlDialect,
    alias_set: HashSet<String>,
    stack: Vec<Frame>,
    correl_table: Vec<(usize, Aliases)>,
}

impl RelToSqlConverter {
    pub fn new(dialect: SqlDialect) -> RelToSqlConverter {
        RelToSqlConverter {
            dialect,
            alias_set: HashSet::new(),
            stack: Vec::new(),
            correl_table: Vec::new(),
        }
    }

    fn is_anon(&self) -> bool {
        self.stack.last().is_none_or(|f| f.anon)
    }

    pub fn visit_root(&mut self, rel: &RelNode) -> Result<SqlResult> {
        let anon = self.is_anon();
        self.visit_input(rel, anon, false, Clauses::default())
    }

    fn visit_input(
        &mut self,
        rel: &RelNode,
        anon: bool,
        ignore_clauses: bool,
        expected: Clauses,
    ) -> Result<SqlResult> {
        self.stack.push(Frame {
            anon,
            ignore_clauses,
            expected_clauses: expected,
        });
        let r = self.dispatch(rel);
        self.stack.pop();
        r
    }

    fn visit_input_clauses(&mut self, rel: &RelNode, expected: Clauses) -> Result<SqlResult> {
        let anon = self.is_anon();
        self.visit_input(rel, anon, false, expected)
    }

    fn make_result(
        &self,
        node: SqlNode,
        clauses: Clauses,
        needed_alias: Option<String>,
        needed_type: Option<RelDataType>,
        aliases: Aliases,
    ) -> SqlResult {
        let (ignore_clauses, expected_clauses) = match self.stack.last() {
            Some(f) => (f.ignore_clauses, f.expected_clauses),
            None => (false, Clauses::default()),
        };
        SqlResult {
            node,
            clauses,
            needed_alias,
            needed_type,
            aliases,
            anon: self.is_anon(),
            ignore_clauses,
            expected_clauses,
        }
    }

    fn result(
        &mut self,
        node: SqlNode,
        clauses: Clauses,
        rel_row_type: &RelDataType,
        aliases: Option<Aliases>,
    ) -> SqlResult {
        let alias2 = node.alias();
        let alias3 = alias2.clone().unwrap_or_else(|| "t".to_string());
        let alias4 = uniquify_expr(&alias3, &mut self.alias_set);
        let row_type = adjusted_row_type(rel_row_type, &node);
        if let Some(a) = aliases {
            if a.len() > 1 {
                return self.make_result(node, clauses, Some(alias4), Some(row_type), a);
            }
        }
        let alias5 = (alias2.as_deref() != Some(alias4.as_str())).then(|| alias4.clone());
        self.make_result(
            node,
            clauses,
            alias5,
            Some(row_type.clone()),
            vec![(alias4, row_type)],
        )
    }

    fn builder(&self, x: &SqlResult, rel: &RelNode) -> Builder {
        let expected = x.expected_clauses;
        let clauses2 = if x.ignore_clauses {
            Clauses::default()
        } else {
            expected
        };
        let need_new = x.need_new_sub_query(rel, &self.dialect, clauses2);
        let (select, clause_list) = if need_new {
            (x.sub_select(&self.dialect), Clauses::default())
        } else {
            (x.to_select(&self.dialect), x.clauses)
        };
        let clause_list = clause_list.union(expected);
        let mut new_aliases: Option<Aliases> = None;
        let context = if is_singleton_star(&select.select_list) {
            let qualified = x.aliases.len() > 1;
            let contains_needed = |a: &str| x.aliases.iter().any(|(k, _)| k == a);
            match &x.needed_alias {
                Some(na) if need_new && (x.aliases.len() != 1 || !contains_needed(na)) => {
                    let input_type = rel
                        .input()
                        .map_or_else(|| rel.row_type().clone(), |i| i.row_type().clone());
                    let na_aliases = vec![(na.clone(), input_type)];
                    new_aliases = Some(na_aliases.clone());
                    Context::alias_context(&self.dialect, na_aliases, qualified)
                }
                _ => Context::alias_context(&self.dialect, x.aliases.clone(), qualified),
            }
        } else {
            Context {
                kind: ContextKind::SelectList {
                    items: select.select_list.clone(),
                },
                dialect: self.dialect,
                correl: Vec::new(),
                subqueries: Vec::new(),
            }
        };
        let contains_needed = match &x.needed_alias {
            Some(na) => x.aliases.iter().any(|(k, _)| k == na),
            None => x.aliases.iter().any(|(k, _)| k.is_empty()),
        };
        let aliases = if need_new && !contains_needed {
            new_aliases
        } else {
            Some(x.aliases.clone())
        };
        let mut context = context;
        context.correl.clone_from(&self.correl_table);
        Builder {
            clauses: clause_list,
            select,
            context,
            anon: self.is_anon(),
            aliases,
            need_new,
        }
    }

    fn builder_result(&mut self, builder: Builder, rel: &RelNode) -> SqlResult {
        let anon = builder.anon;
        let row_type = rel.row_type().clone();
        self.result(
            SqlNode::Select(Box::new(builder.select)),
            builder.clauses,
            &row_type,
            builder.aliases,
        )
        .with_anon(anon)
    }

    fn dispatch(&mut self, rel: &RelNode) -> Result<SqlResult> {
        match rel {
            RelNode::TableScan {
                qualified_name,
                row_type,
            } => {
                let node = SqlNode::Identifier(SqlIdentifier::new(qualified_name.clone()));
                Ok(self.result(node, Clause::From.into(), row_type, None))
            }
            RelNode::Values { row_type, tuples } => self.visit_values(row_type, tuples),
            RelNode::Filter {
                input,
                condition,
                variables_set,
            } => self.visit_filter(rel, input, condition, variables_set),
            RelNode::Join {
                left,
                right,
                condition,
                join_type,
                ..
            } => self.visit_join(left, right, condition, *join_type),
            RelNode::SetOp {
                kind, all, inputs, ..
            } => self.visit_set_op(rel, *kind, *all, inputs),
            RelNode::Project {
                input,
                exprs,
                row_type,
            } => self.visit_project(rel, input, exprs, row_type),
            RelNode::Aggregate {
                input,
                group_set,
                group_sets,
                agg_calls,
                row_type,
            } => self.visit_aggregate(rel, input, group_set, group_sets, agg_calls, row_type),
            RelNode::Sort {
                input,
                collation,
                offset,
                fetch,
            } => self.visit_sort(rel, input, collation, offset.as_ref(), fetch.as_ref()),
        }
    }

    fn visit_values(
        &mut self,
        row_type: &RelDataType,
        tuples: &[Vec<RexLiteral>],
    ) -> Result<SqlResult> {
        let names = row_type.field_names();
        let mut rows = Vec::new();
        if tuples.is_empty() {
            rows.push(SqlNode::call(
                &operators::ROW,
                names
                    .iter()
                    .map(|_| SqlNode::Literal(SqlLiteral::null()))
                    .collect(),
            ));
        } else {
            for t in tuples {
                let mut items = Vec::new();
                for l in t {
                    items.push(literal_to_sql(l)?);
                }
                rows.push(SqlNode::call(&operators::ROW, items));
            }
        }
        let values = SqlNode::call(&VALUES, rows);
        let mut as_operands = vec![values, SqlNode::identifier("t")];
        for n in &names {
            as_operands.push(SqlNode::identifier(n.clone()));
        }
        let mut query = SqlNode::call(&operators::AS, as_operands);
        if tuples.is_empty() {
            let one = SqlNode::Literal(SqlLiteral::exact(BigDecimal::from(1)));
            let zero = SqlNode::Literal(SqlLiteral::exact(BigDecimal::from(0)));
            query = SqlNode::Select(Box::new(SqlSelect {
                select_list: vec![SqlNode::star()],
                from: Some(query),
                where_clause: Some(SqlNode::call(&operators::EQUALS, vec![one, zero])),
                ..SqlSelect::default()
            }));
        }
        Ok(self.result(query, Clause::Select.into(), row_type, None))
    }

    fn parse_correl_table(&mut self, variables_set: &[usize], x: &SqlResult) {
        for id in variables_set {
            self.correl_table.push((*id, x.aliases.clone()));
        }
    }

    fn prepare_sub_queries(&mut self, context: &mut Context, rex: &RexNode) -> Result<()> {
        match rex {
            RexNode::SubQuery(q) => {
                if !context.subqueries.iter().any(|(k, _)| k == q.as_ref()) {
                    let result = self.visit_root(&q.rel)?;
                    let node = result.to_statement(&self.dialect);
                    context.subqueries.push(((**q).clone(), node));
                }
                for o in &q.operands {
                    self.prepare_sub_queries(context, o)?;
                }
            }
            RexNode::Call(c) => {
                for o in &c.operands {
                    self.prepare_sub_queries(context, o)?;
                }
            }
            RexNode::Over(o) => {
                for x in &o.operands {
                    self.prepare_sub_queries(context, x)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn visit_join(
        &mut self,
        left: &RelNode,
        right: &RelNode,
        condition: &RexNode,
        join_type: JoinType,
    ) -> Result<SqlResult> {
        let anon = self.is_anon();
        let left_result = self
            .visit_input(left, anon, false, Clauses::default())?
            .reset_alias();
        let anon = self.is_anon();
        let right_result = self
            .visit_input(right, anon, false, Clauses::default())?
            .reset_alias();
        let mut aliases = left_result.aliases.clone();
        aliases.extend(right_result.aliases.iter().cloned());
        let (op, sql_condition) = if join_type == JoinType::Inner && condition.is_always_true() {
            let comma = !matches!(right, RelNode::Join { .. });
            (
                if comma {
                    &operators::COMMA_JOIN
                } else {
                    &operators::CROSS_JOIN
                },
                None,
            )
        } else {
            let op: &'static SqlOperator = match join_type {
                JoinType::Inner => &operators::INNER_JOIN,
                JoinType::Left => &operators::LEFT_JOIN,
                JoinType::Right => &operators::RIGHT_JOIN,
                JoinType::Full => &operators::FULL_JOIN,
                JoinType::Semi | JoinType::Anti => {
                    return Err(SubstraitError::Unsupported(format!(
                        "join type {join_type:?} is not supported"
                    )))
                }
            };
            let mut context = Context::alias_context(&self.dialect, aliases.clone(), true);
            context.correl.clone_from(&self.correl_table);
            self.prepare_sub_queries(&mut context, condition)?;
            let sql = if condition.is_always_true() {
                SqlNode::Literal(SqlLiteral::boolean(true))
            } else if condition.is_always_false() {
                SqlNode::Literal(SqlLiteral::boolean(false))
            } else {
                context.to_sql(condition)?
            };
            (op, Some(sql))
        };
        let mut operands = vec![left_result.to_from(), right_result.to_from()];
        operands.extend(sql_condition);
        let join = SqlNode::call(op, operands);
        Ok(self.make_result(join, Clause::From.into(), None, None, aliases))
    }

    fn visit_set_op(
        &mut self,
        rel: &RelNode,
        kind: SetOpKind,
        all: bool,
        inputs: &[RelNode],
    ) -> Result<SqlResult> {
        let op: &'static SqlOperator = match (kind, all) {
            (SetOpKind::Union, true) => &operators::UNION_ALL,
            (SetOpKind::Union, false) => &operators::UNION,
            (SetOpKind::Intersect, true) => &operators::INTERSECT_ALL,
            (SetOpKind::Intersect, false) => &operators::INTERSECT,
            (SetOpKind::Minus, true) => &operators::EXCEPT_ALL,
            (SetOpKind::Minus, false) => &operators::EXCEPT,
        };
        let mut node: Option<SqlNode> = None;
        for input in inputs {
            let anon = self.is_anon();
            let result = self.visit_input(input, anon, false, Clauses::default())?;
            let select = SqlNode::Select(Box::new(result.to_select(&self.dialect)));
            node = Some(match node {
                None => select,
                Some(n) => SqlNode::call(op, vec![n, select]),
            });
        }
        let node = node.ok_or_else(|| {
            SubstraitError::IllegalState("set op must have at least one input".into())
        })?;
        let row_type = rel.row_type().clone();
        Ok(self.result(node, Clause::SetOp.into(), &row_type, None))
    }

    fn visit_filter(
        &mut self,
        rel: &RelNode,
        input: &RelNode,
        condition: &RexNode,
        variables_set: &[usize],
    ) -> Result<SqlResult> {
        if let RelNode::Aggregate {
            input: agg_input, ..
        } = input
        {
            let ignore = agg_input.is_project();
            let anon = self.is_anon();
            let x = self.visit_input(input, anon, ignore, Clause::Having.into())?;
            self.parse_correl_table(variables_set, &x);
            let mut builder = self.builder(&x, rel);
            self.prepare_sub_queries(&mut builder.context, condition)?;
            let cond = builder.context.to_sql(condition)?;
            if builder.need_new {
                if let Some(inner) = inner_select_mut(&mut builder.select) {
                    let having = and_expressions(inner.having.take(), cond);
                    inner.having = Some(having);
                }
            } else {
                let having = and_expressions(builder.select.having.take(), cond);
                builder.select.having = Some(having);
            }
            return Ok(self.builder_result(builder, rel));
        }
        let mut x = self.visit_input_clauses(input, Clause::Where.into())?;
        if matches!(
            condition.kind(),
            SqlKind::Not | SqlKind::Exists | SqlKind::In
        ) {
            x = x.reset_alias();
        }
        self.parse_correl_table(variables_set, &x);
        let mut builder = self.builder(&x, rel);
        self.prepare_sub_queries(&mut builder.context, condition)?;
        let w = builder.context.to_sql(condition)?;
        builder.select.where_clause = Some(w);
        Ok(self.builder_result(builder, rel))
    }

    fn visit_project(
        &mut self,
        rel: &RelNode,
        input: &RelNode,
        exprs: &[RexNode],
        row_type: &RelDataType,
    ) -> Result<SqlResult> {
        let x = if input.is_sort() {
            self.visit_input_clauses(input, Clauses::default())?
        } else {
            self.visit_input_clauses(input, Clause::Select.into())?
        };
        let mut builder = self.builder(&x, rel);
        if !is_star(exprs, input.row_type(), row_type) {
            let mut select_list = Vec::new();
            for e in exprs {
                self.prepare_sub_queries(&mut builder.context, e)?;
                let mut sql = builder.context.to_sql(e)?;
                if sql.is_null_literal() {
                    let field_type = row_type
                        .field_list()
                        .get(select_list.len())
                        .map(|f| f.ty().clone());
                    if let Some(ft) = field_type {
                        if let Some(spec) = self.dialect.cast_spec(&ft) {
                            sql =
                                SqlNode::call(&operators::CAST, vec![sql, SqlNode::DataType(spec)]);
                        }
                    }
                }
                add_select(&mut select_list, sql, row_type);
            }
            if select_list.is_empty() {
                select_list.push(SqlNode::Literal(SqlLiteral::exact(BigDecimal::from(1))));
            }
            if builder.select.group_by.is_none()
                && builder.select.having.is_none()
                && contains_agg_list(&builder.select.select_list)
                && !contains_agg_list(&select_list)
            {
                builder.select.group_by = Some(Vec::new());
            }
            builder.select.select_list = select_list;
        }
        Ok(self.builder_result(builder, rel))
    }

    fn visit_aggregate(
        &mut self,
        rel: &RelNode,
        input: &RelNode,
        group_set: &[usize],
        group_sets: &[Vec<usize>],
        agg_calls: &[AggregateCall],
        row_type: &RelDataType,
    ) -> Result<SqlResult> {
        let ignore = input.is_project();
        let anon = self.is_anon();
        let x = self.visit_input(input, anon, ignore, Clause::GroupBy.into())?;
        let mut builder = self.builder(&x, rel);
        let mut select_list = Vec::new();
        let mut group_keys = Vec::new();
        for key in group_set {
            group_keys.push(builder.context.field(*key)?);
        }
        let mut sorted = group_set.to_vec();
        sorted.sort_unstable();
        for key in &sorted {
            let f = builder.context.field(*key)?;
            add_select(&mut select_list, f, row_type);
        }
        let group_by_list =
            generate_group_list(&builder.context, group_keys, group_set, group_sets)?;
        for call in agg_calls {
            let mut node = builder.context.agg_to_sql(call)?;
            if matches!(call.op.kind, SqlKind::Min | SqlKind::Max) {
                node = self.rewrite_max_min(node, call);
            }
            add_select(&mut select_list, node, row_type);
        }
        builder.select.select_list = select_list;
        if !group_by_list.is_empty() || agg_calls.is_empty() {
            builder.select.group_by = Some(group_by_list);
        }
        Ok(self.builder_result(builder, rel))
    }

    fn rewrite_max_min(&self, node: SqlNode, call: &AggregateCall) -> SqlNode {
        if call.ty.sql_type_name() != SqlTypeName::Boolean {
            return node;
        }
        let is_max = call.op.kind == SqlKind::Max;
        let op: &'static SqlOperator = match self.dialect.database_product() {
            super::sql_dialect::DatabaseProduct::Postgresql => {
                if is_max {
                    &operators::BOOL_OR
                } else {
                    &operators::BOOL_AND
                }
            }
            super::sql_dialect::DatabaseProduct::Snowflake => {
                if is_max {
                    &operators::BOOLOR_AGG
                } else {
                    &operators::BOOLAND_AGG
                }
            }
            super::sql_dialect::DatabaseProduct::Unknown => return node,
        };
        match node {
            SqlNode::Call(c) => SqlNode::call(op, c.operands.into_iter().take(1).collect()),
            other => other,
        }
    }

    fn visit_sort(
        &mut self,
        rel: &RelNode,
        input: &RelNode,
        collation: &[RelFieldCollation],
        offset: Option<&RexNode>,
        fetch: Option<&RexNode>,
    ) -> Result<SqlResult> {
        let x = self.visit_input_clauses(
            input,
            Clauses::of(&[Clause::OrderBy, Clause::Offset, Clause::Fetch]),
        )?;
        let mut builder = self.builder(&x, rel);
        if self.stack.len() != 1 && is_singleton_star(&builder.select.select_list) {
            let mut list = Vec::new();
            for i in 0..rel.row_type().field_count() {
                let f = builder.context.field(i)?;
                add_select(&mut list, f, rel.row_type());
            }
            builder.select.select_list = list;
        }
        let mut order_by = Vec::new();
        for c in collation {
            builder.add_order_item(&mut order_by, c)?;
        }
        if !order_by.is_empty() {
            builder.select.order_by = Some(order_by);
        }
        if let Some(f) = fetch {
            let n = builder.context.to_sql(f)?;
            builder.select.fetch = Some(n);
        }
        if let Some(o) = offset {
            let n = builder.context.to_sql(o)?;
            builder.select.offset = Some(n);
        }
        Ok(self.builder_result(builder, rel))
    }
}

fn is_rollup(group_set: &[usize], group_sets: &[Vec<usize>]) -> bool {
    if group_sets.len() != group_set.len() + 1 {
        return false;
    }
    let mut previous: Option<&Vec<usize>> = None;
    for set in group_sets {
        match previous {
            None => {
                if set.as_slice() != group_set {
                    return false;
                }
            }
            Some(g) => {
                if !set.iter().all(|b| g.contains(b)) || g.len() - set.len() != 1 {
                    return false;
                }
            }
        }
        previous = Some(set);
    }
    true
}

fn rollup_bits(group_sets: &[Vec<usize>]) -> Vec<usize> {
    let mut bits: Vec<usize> = Vec::new();
    let mut previous: Option<&Vec<usize>> = None;
    for set in group_sets {
        if let Some(g) = previous {
            for b in g {
                if !set.contains(b) && !bits.contains(b) {
                    bits.push(*b);
                }
            }
        }
        previous = Some(set);
    }
    bits.reverse();
    bits
}

fn generate_group_list(
    context: &Context,
    group_keys: Vec<SqlNode>,
    group_set: &[usize],
    group_sets: &[Vec<usize>],
) -> Result<Vec<SqlNode>> {
    let mut whole: Vec<usize> = group_set.to_vec();
    whole.sort_unstable();
    if group_sets.len() == 1 && group_sets[0] == whole {
        return Ok(group_keys);
    }
    let cube = u32::try_from(whole.len())
        .ok()
        .and_then(|n| 2usize.checked_pow(n))
        .is_some_and(|n| n == group_sets.len());
    if cube && whole.len() > 1 {
        return Ok(vec![SqlNode::call(&operators::CUBE, group_keys)]);
    }
    if cube || is_rollup(&whole, group_sets) {
        let mut keys = Vec::new();
        for bit in rollup_bits(group_sets) {
            keys.push(context.field(bit)?);
        }
        return Ok(vec![SqlNode::call(&operators::ROLLUP, keys)]);
    }
    let mut sets: Vec<Vec<usize>> = Vec::new();
    let mut union: Vec<usize> = group_sets.iter().flatten().copied().collect();
    union.sort_unstable();
    union.dedup();
    if union != whole {
        sets.push(whole.clone());
    }
    sets.extend(group_sets.iter().cloned());
    let items = sets
        .iter()
        .map(|set| {
            let nodes: Vec<SqlNode> = set
                .iter()
                .filter_map(|key| {
                    whole
                        .iter()
                        .position(|w| w == key)
                        .and_then(|i| group_keys.get(i).cloned())
                })
                .collect();
            if nodes.len() == 1 {
                nodes
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| SqlNode::NodeList(Vec::new()))
            } else {
                SqlNode::call(&operators::ROW, nodes)
            }
        })
        .collect();
    Ok(vec![SqlNode::call(&operators::GROUPING_SETS, items)])
}

fn inner_select_mut(select: &mut SqlSelect) -> Option<&mut SqlSelect> {
    match select.from.as_mut()? {
        SqlNode::Select(s) => Some(s),
        SqlNode::Call(c) if c.op.kind == SqlKind::As => match c.operands.first_mut()? {
            SqlNode::Select(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

pub static VALUES: SqlOperator = SqlOperator {
    name: "VALUES",
    kind: SqlKind::Values,
    left_prec: 2,
    right_prec: 3,
    syntax: operators::SqlSyntax::Special,
    aggregate: false,
    negated: false,
};
