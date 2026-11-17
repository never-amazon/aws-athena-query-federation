use bigdecimal::BigDecimal;

use super::operators::{self, SqlKind, SqlOperator};
use super::rel::RelNode;
use super::rel_data_type::{RelDataType, RelDataTypeSystem};
use super::sarg::{Sarg, SargValue};
use super::sql_type_name::SqlTypeName;
use super::values::{BitString, DateString, TimeString, TimestampString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    Year,
    Quarter,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
    Dow,
    Doy,
    Epoch,
}

impl TimeUnit {
    pub fn name(self) -> &'static str {
        match self {
            TimeUnit::Year => "YEAR",
            TimeUnit::Quarter => "QUARTER",
            TimeUnit::Month => "MONTH",
            TimeUnit::Week => "WEEK",
            TimeUnit::Day => "DAY",
            TimeUnit::Hour => "HOUR",
            TimeUnit::Minute => "MINUTE",
            TimeUnit::Second => "SECOND",
            TimeUnit::Millisecond => "MILLISECOND",
            TimeUnit::Microsecond => "MICROSECOND",
            TimeUnit::Nanosecond => "NANOSECOND",
            TimeUnit::Dow => "DOW",
            TimeUnit::Doy => "DOY",
            TimeUnit::Epoch => "EPOCH",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrimFlag {
    Both,
    Leading,
    Trailing,
}

impl TrimFlag {
    pub fn name(self) -> &'static str {
        match self {
            TrimFlag::Both => "BOTH",
            TrimFlag::Leading => "LEADING",
            TrimFlag::Trailing => "TRAILING",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Symbol {
    TimeUnit(TimeUnit),
    Trim(TrimFlag),
}

#[derive(Debug, Clone, PartialEq)]
pub enum LiteralValue {
    Null,
    Boolean(bool),
    Exact(BigDecimal),
    Approx(f64),
    Char(String),
    Binary(BitString),
    Date(DateString),
    Time(TimeString),
    Timestamp(TimestampString),
    Interval(BigDecimal),
    Symbol(Symbol),
    Sarg(Box<Sarg>),
}

impl LiteralValue {
    pub fn to_sarg_value(&self) -> Option<SargValue> {
        match self {
            LiteralValue::Boolean(b) => Some(SargValue::Boolean(*b)),
            LiteralValue::Exact(d) => Some(SargValue::Exact(d.clone())),
            LiteralValue::Approx(f) => Some(SargValue::Approx(*f)),
            LiteralValue::Char(s) => Some(SargValue::Char(s.clone())),
            LiteralValue::Binary(b) => Some(SargValue::Binary(b.clone())),
            LiteralValue::Date(d) => Some(SargValue::Date(d.clone())),
            LiteralValue::Time(t) => Some(SargValue::Time(t.clone())),
            LiteralValue::Timestamp(t) => Some(SargValue::Timestamp(t.clone())),
            _ => None,
        }
    }

    pub fn from_sarg_value(v: &SargValue) -> LiteralValue {
        match v {
            SargValue::Boolean(b) => LiteralValue::Boolean(*b),
            SargValue::Exact(d) => LiteralValue::Exact(d.clone()),
            SargValue::Approx(f) => LiteralValue::Approx(*f),
            SargValue::Char(s) => LiteralValue::Char(s.clone()),
            SargValue::Binary(b) => LiteralValue::Binary(b.clone()),
            SargValue::Date(d) => LiteralValue::Date(d.clone()),
            SargValue::Time(t) => LiteralValue::Time(t.clone()),
            SargValue::Timestamp(t) => LiteralValue::Timestamp(t.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexLiteral {
    pub value: LiteralValue,
    pub ty: RelDataType,
}

impl RexLiteral {
    pub fn is_null(&self) -> bool {
        matches!(self.value, LiteralValue::Null)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexCall {
    pub op: &'static SqlOperator,
    pub operands: Vec<RexNode>,
    pub ty: RelDataType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullDirection {
    First,
    Last,
    Unspecified,
}

impl Direction {
    pub fn default_null_direction(self) -> NullDirection {
        match self {
            Direction::Ascending => NullDirection::Last,
            Direction::Descending => NullDirection::First,
        }
    }

    pub fn is_descending(self) -> bool {
        self == Direction::Descending
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexFieldCollation {
    pub expr: RexNode,
    pub direction: Direction,
    pub null_direction: NullDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RexWindowBound {
    UnboundedPreceding,
    UnboundedFollowing,
    CurrentRow,
    Preceding(RexNode),
    Following(RexNode),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexWindow {
    pub partition_keys: Vec<RexNode>,
    pub order_keys: Vec<RexFieldCollation>,
    pub lower_bound: RexWindowBound,
    pub upper_bound: RexWindowBound,
    pub is_rows: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexOver {
    pub op: &'static SqlOperator,
    pub operands: Vec<RexNode>,
    pub ty: RelDataType,
    pub window: RexWindow,
    pub distinct: bool,
    pub ignore_nulls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubQueryKind {
    In,
    Exists,
    Unique,
    Scalar,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RexSubQuery {
    pub kind: SubQueryKind,
    pub operands: Vec<RexNode>,
    pub rel: RelNode,
    pub ty: RelDataType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RexNode {
    InputRef {
        index: usize,
        ty: RelDataType,
    },
    Literal(RexLiteral),
    Call(RexCall),
    Over(Box<RexOver>),
    SubQuery(Box<RexSubQuery>),
    CorrelField {
        correl: usize,
        index: usize,
        ty: RelDataType,
    },
}

impl RexNode {
    pub fn get_type(&self) -> &RelDataType {
        match self {
            RexNode::InputRef { ty, .. } | RexNode::CorrelField { ty, .. } => ty,
            RexNode::Literal(l) => &l.ty,
            RexNode::Call(c) => &c.ty,
            RexNode::Over(o) => &o.ty,
            RexNode::SubQuery(q) => &q.ty,
        }
    }

    pub fn get_kind(&self) -> SqlKind {
        match self {
            RexNode::InputRef { .. } => SqlKind::InputRef,
            RexNode::Literal(_) => SqlKind::Literal,
            RexNode::Call(c) => c.op.kind,
            RexNode::Over(o) => o.op.kind,
            RexNode::SubQuery(q) => match q.kind {
                SubQueryKind::In => SqlKind::In,
                SubQueryKind::Exists => SqlKind::Exists,
                SubQueryKind::Unique => SqlKind::Unique,
                SubQueryKind::Scalar => SqlKind::ScalarQuery,
            },
            RexNode::CorrelField { .. } => SqlKind::FieldAccess,
        }
    }

    pub fn as_call(&self) -> Option<&RexCall> {
        match self {
            RexNode::Call(c) => Some(c),
            _ => None,
        }
    }

    pub fn input_ref_index(&self) -> Option<usize> {
        match self {
            RexNode::InputRef { index, .. } => Some(*index),
            _ => None,
        }
    }

    pub fn operands(&self) -> &[RexNode] {
        match self {
            RexNode::Call(c) => &c.operands,
            RexNode::Over(o) => &o.operands,
            RexNode::SubQuery(q) => &q.operands,
            _ => &[],
        }
    }

    pub fn is_always_true(&self) -> bool {
        match self {
            RexNode::Literal(RexLiteral {
                value: LiteralValue::Boolean(b),
                ..
            }) => *b,
            RexNode::Call(c) => match c.op.kind {
                SqlKind::IsNotNull => !c.operands[0].get_type().is_nullable(),
                SqlKind::IsNotTrue | SqlKind::IsFalse | SqlKind::Not => {
                    c.operands[0].is_always_false()
                }
                SqlKind::IsNotFalse | SqlKind::IsTrue | SqlKind::Cast => {
                    c.operands[0].is_always_true()
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub fn is_always_false(&self) -> bool {
        match self {
            RexNode::Literal(RexLiteral {
                value: LiteralValue::Boolean(b),
                ..
            }) => !*b,
            RexNode::Call(c) => match c.op.kind {
                SqlKind::IsNull => !c.operands[0].get_type().is_nullable(),
                SqlKind::IsNotTrue | SqlKind::IsFalse | SqlKind::Not => {
                    c.operands[0].is_always_true()
                }
                SqlKind::IsNotFalse | SqlKind::IsTrue | SqlKind::Cast => {
                    c.operands[0].is_always_false()
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub fn is_null_literal(&self) -> bool {
        match self {
            RexNode::Literal(l) => l.is_null(),
            _ => false,
        }
    }

    pub fn node_count(&self) -> usize {
        match self {
            RexNode::Call(c) => 1 + c.operands.iter().map(RexNode::node_count).sum::<usize>(),
            RexNode::Over(o) => 1 + o.operands.iter().map(RexNode::node_count).sum::<usize>(),
            _ => 1,
        }
    }

    pub fn contains_over(&self) -> bool {
        match self {
            RexNode::Over(_) => true,
            RexNode::Call(c) => c.operands.iter().any(RexNode::contains_over),
            _ => false,
        }
    }

    pub fn collect_input_refs(&self, out: &mut Vec<usize>) {
        match self {
            RexNode::InputRef { index, .. } => {
                if !out.contains(index) {
                    out.push(*index);
                }
            }
            RexNode::Call(c) => c.operands.iter().for_each(|o| o.collect_input_refs(out)),
            RexNode::Over(o) => {
                o.operands.iter().for_each(|x| x.collect_input_refs(out));
                o.window
                    .partition_keys
                    .iter()
                    .for_each(|x| x.collect_input_refs(out));
                o.window
                    .order_keys
                    .iter()
                    .for_each(|x| x.expr.collect_input_refs(out));
            }
            RexNode::SubQuery(q) => q.operands.iter().for_each(|o| o.collect_input_refs(out)),
            RexNode::Literal(_) | RexNode::CorrelField { .. } => {}
        }
    }

    pub fn shift_refs(&self, map: &dyn Fn(usize) -> RexNode) -> RexNode {
        match self {
            RexNode::InputRef { index, .. } => map(*index),
            RexNode::Literal(_) | RexNode::CorrelField { .. } => self.clone(),
            RexNode::SubQuery(q) => RexNode::SubQuery(Box::new(RexSubQuery {
                kind: q.kind,
                operands: q.operands.iter().map(|o| o.shift_refs(map)).collect(),
                rel: q.rel.clone(),
                ty: q.ty.clone(),
            })),
            RexNode::Call(c) => RexNode::Call(RexCall {
                op: c.op,
                operands: c.operands.iter().map(|o| o.shift_refs(map)).collect(),
                ty: c.ty.clone(),
            }),
            RexNode::Over(o) => RexNode::Over(Box::new(RexOver {
                op: o.op,
                operands: o.operands.iter().map(|x| x.shift_refs(map)).collect(),
                ty: o.ty.clone(),
                window: RexWindow {
                    partition_keys: o
                        .window
                        .partition_keys
                        .iter()
                        .map(|x| x.shift_refs(map))
                        .collect(),
                    order_keys: o
                        .window
                        .order_keys
                        .iter()
                        .map(|k| RexFieldCollation {
                            expr: k.expr.shift_refs(map),
                            direction: k.direction,
                            null_direction: k.null_direction,
                        })
                        .collect(),
                    lower_bound: o.window.lower_bound.clone(),
                    upper_bound: o.window.upper_bound.clone(),
                    is_rows: o.window.is_rows,
                },
                distinct: o.distinct,
                ignore_nulls: o.ignore_nulls,
            })),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RexBuilder {
    pub system: RelDataTypeSystem,
}

impl RexBuilder {
    pub fn new(system: RelDataTypeSystem) -> RexBuilder {
        RexBuilder { system }
    }

    pub fn boolean_type(self, nullable: bool) -> RelDataType {
        RelDataType::create_sql_type(self.system, SqlTypeName::Boolean).with_nullability(nullable)
    }

    pub fn make_input_ref(index: usize, ty: RelDataType) -> RexNode {
        RexNode::InputRef { index, ty }
    }

    pub fn make_bool_literal(self, b: bool) -> RexNode {
        RexNode::Literal(RexLiteral {
            value: LiteralValue::Boolean(b),
            ty: self.boolean_type(false),
        })
    }

    pub fn make_null_literal(ty: &RelDataType) -> RexNode {
        RexNode::Literal(RexLiteral {
            value: LiteralValue::Null,
            ty: ty.with_nullability(true),
        })
    }

    pub fn make_literal(value: LiteralValue, ty: RelDataType) -> RexNode {
        if matches!(value, LiteralValue::Null) {
            return RexBuilder::make_null_literal(&ty);
        }
        RexNode::Literal(RexLiteral { value, ty })
    }

    pub fn make_exact_int(self, v: i64) -> RexNode {
        let ty = if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(&v) {
            RelDataType::create_sql_type(self.system, SqlTypeName::Integer)
        } else {
            RelDataType::create_sql_type(self.system, SqlTypeName::Bigint)
        };
        RexNode::Literal(RexLiteral {
            value: LiteralValue::Exact(BigDecimal::from(v)),
            ty,
        })
    }

    pub fn make_bigint_literal(self, v: i64) -> RexNode {
        RexNode::Literal(RexLiteral {
            value: LiteralValue::Exact(BigDecimal::from(v)),
            ty: RelDataType::create_sql_type(self.system, SqlTypeName::Bigint),
        })
    }

    pub fn infer_type(self, op: &'static SqlOperator, operands: &[RexNode]) -> RelDataType {
        let any_nullable = operands.iter().any(|o| o.get_type().is_nullable());
        match op.kind {
            SqlKind::IsNull
            | SqlKind::IsNotNull
            | SqlKind::IsTrue
            | SqlKind::IsNotTrue
            | SqlKind::IsFalse
            | SqlKind::IsNotFalse
            | SqlKind::IsDistinctFrom
            | SqlKind::IsNotDistinctFrom => self.boolean_type(false),
            SqlKind::Search => {
                let nullable = operands.first().is_some_and(|o| o.get_type().is_nullable())
                    && match operands.get(1) {
                        Some(RexNode::Literal(RexLiteral {
                            value: LiteralValue::Sarg(s),
                            ..
                        })) => s.null_as == super::sarg::RexUnknownAs::Unknown,
                        _ => true,
                    };
                self.boolean_type(nullable)
            }
            SqlKind::And
            | SqlKind::Or
            | SqlKind::Not
            | SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThan
            | SqlKind::LessThanOrEqual
            | SqlKind::Like
            | SqlKind::In
            | SqlKind::NotIn
            | SqlKind::Between => self.boolean_type(any_nullable),
            _ => operands.first().map_or_else(
                || self.boolean_type(any_nullable),
                |o| o.get_type().with_nullability(any_nullable),
            ),
        }
    }

    pub fn make_call(self, op: &'static SqlOperator, operands: Vec<RexNode>) -> RexNode {
        let ty = self.infer_type(op, &operands);
        RexNode::Call(RexCall { op, operands, ty })
    }

    pub fn make_call_typed(
        ty: RelDataType,
        op: &'static SqlOperator,
        operands: Vec<RexNode>,
    ) -> RexNode {
        RexNode::Call(RexCall { op, operands, ty })
    }

    pub fn make_search(self, ref_node: RexNode, sarg: Sarg, ty: RelDataType) -> RexNode {
        let literal = RexNode::Literal(RexLiteral {
            value: LiteralValue::Sarg(Box::new(sarg)),
            ty,
        });
        self.make_call(&operators::SEARCH, vec![ref_node, literal])
    }

    pub fn make_cast(ty: RelDataType, operand: RexNode, safe: bool) -> RexNode {
        let op = if safe {
            &operators::SAFE_CAST
        } else {
            &operators::CAST
        };
        RexNode::Call(RexCall {
            op,
            operands: vec![operand],
            ty,
        })
    }

    pub fn make_not(self, operand: RexNode) -> RexNode {
        self.make_call(&operators::NOT, vec![operand])
    }
}

pub fn flatten(kind: SqlKind, node: &RexNode, out: &mut Vec<RexNode>) {
    match node {
        RexNode::Call(c) if c.op.kind == kind => {
            for o in &c.operands {
                flatten(kind, o, out);
            }
        }
        other => out.push(other.clone()),
    }
}

pub fn conjunctions(node: &RexNode) -> Vec<RexNode> {
    let mut out = Vec::new();
    flatten(SqlKind::And, node, &mut out);
    out
}

pub fn disjunctions(node: &RexNode) -> Vec<RexNode> {
    let mut out = Vec::new();
    flatten(SqlKind::Or, node, &mut out);
    out
}

pub fn compose_conjunction(builder: RexBuilder, nodes: Vec<RexNode>) -> RexNode {
    let mut list: Vec<RexNode> = Vec::new();
    for n in nodes {
        let mut flat = Vec::new();
        flatten(SqlKind::And, &n, &mut flat);
        for f in flat {
            if f.is_always_true() {
                continue;
            }
            if !list.contains(&f) {
                list.push(f);
            }
        }
    }
    match list.len() {
        0 => builder.make_bool_literal(true),
        1 => list
            .pop()
            .unwrap_or_else(|| builder.make_bool_literal(true)),
        _ => {
            if list.iter().any(RexNode::is_always_false) {
                return builder.make_bool_literal(false);
            }
            builder.make_call(&operators::AND, list)
        }
    }
}

pub fn compose_disjunction(builder: RexBuilder, nodes: Vec<RexNode>) -> RexNode {
    let mut list: Vec<RexNode> = Vec::new();
    for n in nodes {
        let mut flat = Vec::new();
        flatten(SqlKind::Or, &n, &mut flat);
        for f in flat {
            if f.is_always_false() {
                continue;
            }
            if !list.contains(&f) {
                list.push(f);
            }
        }
    }
    match list.len() {
        0 => builder.make_bool_literal(false),
        1 => list
            .pop()
            .unwrap_or_else(|| builder.make_bool_literal(false)),
        _ => {
            if list.iter().any(RexNode::is_always_true) {
                return builder.make_bool_literal(true);
            }
            builder.make_call(&operators::OR, list)
        }
    }
}
