use std::cmp::Ordering;

use bigdecimal::BigDecimal;

use super::operators::{self, SqlKind, SqlOperator};
use super::rel_data_type::RelDataType;
use super::rex::{
    compose_conjunction, compose_disjunction, conjunctions, disjunctions, LiteralValue, RexBuilder,
    RexCall, RexLiteral, RexNode,
};
use super::sarg::{Range, RangeSet, RexUnknownAs, Sarg, SargValue};
use super::sql_type_name::{SqlTypeFamily, SqlTypeName};

#[derive(Debug, Clone, Copy)]
pub struct RexSimplify {
    builder: RexBuilder,
    predicate_elimination: bool,
}

#[derive(Debug, Clone)]
struct SargBuilder {
    ref_node: RexNode,
    types: Vec<RelDataType>,
    range_set: RangeSet,
    has_sarg: bool,
    merged_sarg: bool,
    null_as: RexUnknownAs,
}

impl SargBuilder {
    fn new(ref_node: RexNode) -> SargBuilder {
        SargBuilder {
            ref_node,
            types: Vec::new(),
            range_set: RangeSet::empty(),
            has_sarg: false,
            merged_sarg: false,
            null_as: RexUnknownAs::False,
        }
    }

    fn build(&self, negate: bool) -> Sarg {
        if negate {
            Sarg::of(self.null_as.negate(), self.range_set.complement())
        } else {
            Sarg::of(self.null_as, self.range_set.clone())
        }
    }

    fn add_range(&mut self, range: Range, ty: &RelDataType) {
        self.types.push(ty.clone());
        self.range_set.add(range);
        self.merged_sarg |= self.has_sarg;
        self.null_as = self.null_as.or(RexUnknownAs::Unknown);
    }

    fn add_sarg(&mut self, sarg: &Sarg, negate: bool, ty: &RelDataType) {
        let (r, null_as) = if negate {
            (sarg.range_set.complement(), sarg.null_as.negate())
        } else {
            (sarg.range_set.clone(), sarg.null_as)
        };
        self.types.push(ty.clone());
        self.range_set.add_all(&r);
        self.merged_sarg |= !self.range_set.is_empty();
        self.has_sarg = true;
        self.null_as = self.null_as.or(null_as);
    }

    fn get_type(&self) -> RelDataType {
        match self.types.first() {
            None => self.ref_node.get_type().clone(),
            Some(first) => {
                let nullable = self.types.iter().any(RelDataType::is_nullable);
                let mut best = first.clone();
                for t in &self.types {
                    if t.sql_type_name() == best.sql_type_name() {
                        if t.precision() > best.precision() {
                            best = t.clone();
                        }
                    } else if t.sql_type_name() == SqlTypeName::Varchar {
                        best = t.clone();
                    }
                }
                best.with_nullability(nullable)
            }
        }
    }
}

#[derive(Debug, Clone)]
enum CollectedTerm {
    Node(RexNode),
    Sarg(usize),
}

struct SargCollector {
    builders: Vec<SargBuilder>,
    negate: bool,
    new_terms_count: usize,
}

impl SargCollector {
    fn new(negate: bool) -> SargCollector {
        SargCollector {
            builders: Vec::new(),
            negate,
            new_terms_count: 0,
        }
    }

    fn builder_for(&mut self, e: &RexNode, new_terms: &mut Vec<CollectedTerm>) -> usize {
        if let Some(i) = self.builders.iter().position(|b| &b.ref_node == e) {
            return i;
        }
        self.builders.push(SargBuilder::new(e.clone()));
        let i = self.builders.len() - 1;
        new_terms.push(CollectedTerm::Sarg(i));
        i
    }

    fn accept(&mut self, term: &RexNode, new_terms: &mut Vec<CollectedTerm>) {
        if !self.accept_inner(term, new_terms) {
            new_terms.push(CollectedTerm::Node(term.clone()));
        }
        self.new_terms_count = new_terms.len();
    }

    fn accept_inner(&mut self, e: &RexNode, new_terms: &mut Vec<CollectedTerm>) -> bool {
        let Some(call) = e.as_call() else {
            return false;
        };
        match call.op.kind {
            SqlKind::LessThan
            | SqlKind::LessThanOrEqual
            | SqlKind::GreaterThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::Search => self.accept2(
                &call.operands[0],
                &call.operands[1],
                call.op.kind,
                new_terms,
            ),
            SqlKind::IsNull | SqlKind::IsNotNull => {
                self.accept1(&call.operands[0], call.op.kind, new_terms)
            }
            _ => false,
        }
    }

    fn accept2(
        &mut self,
        left: &RexNode,
        right: &RexNode,
        kind: SqlKind,
        new_terms: &mut Vec<CollectedTerm>,
    ) -> bool {
        let left_ok = matches!(left.kind(), SqlKind::InputRef | SqlKind::Cast);
        let right_ok = matches!(right.kind(), SqlKind::InputRef | SqlKind::Cast);
        match (left, right) {
            (_, RexNode::Literal(lit)) if left_ok => self.accept2b(left, kind, lit, new_terms),
            (RexNode::Literal(lit), _) if right_ok => {
                self.accept2b(right, kind.reverse(), lit, new_terms)
            }
            _ => false,
        }
    }

    fn accept1(&mut self, e: &RexNode, kind: SqlKind, new_terms: &mut Vec<CollectedTerm>) -> bool {
        let i = self.builder_for(e, new_terms);
        let k = if self.negate {
            match kind {
                SqlKind::IsNull => SqlKind::IsNotNull,
                _ => SqlKind::IsNull,
            }
        } else {
            kind
        };
        let b = &mut self.builders[i];
        if k == SqlKind::IsNull {
            b.null_as = b.null_as.or(RexUnknownAs::True);
        } else {
            b.null_as = b.null_as.or(RexUnknownAs::False);
            b.range_set.add(Range::all());
        }
        true
    }

    fn accept2b(
        &mut self,
        e: &RexNode,
        kind: SqlKind,
        literal: &RexLiteral,
        new_terms: &mut Vec<CollectedTerm>,
    ) -> bool {
        if literal.is_null() {
            return false;
        }
        if let LiteralValue::Sarg(sarg) = &literal.value {
            let i = self.builder_for(e, new_terms);
            let negate = self.negate;
            self.builders[i].add_sarg(sarg, negate, &literal.ty);
            return true;
        }
        let Some(value) = literal.value.to_sarg_value() else {
            return false;
        };
        let i = self.builder_for(e, new_terms);
        let kind = if self.negate {
            kind.negate_null_safe()
        } else {
            kind
        };
        let b = &mut self.builders[i];
        match kind {
            SqlKind::LessThan => b.add_range(Range::less_than(value), &literal.ty),
            SqlKind::LessThanOrEqual => b.add_range(Range::at_most(value), &literal.ty),
            SqlKind::GreaterThan => b.add_range(Range::greater_than(value), &literal.ty),
            SqlKind::GreaterThanOrEqual => b.add_range(Range::at_least(value), &literal.ty),
            SqlKind::Equals => b.add_range(Range::singleton(value), &literal.ty),
            SqlKind::NotEquals => {
                b.add_range(Range::less_than(value.clone()), &literal.ty);
                b.add_range(Range::greater_than(value), &literal.ty);
            }
            _ => return false,
        }
        true
    }

    fn need_to_fix(&self) -> bool {
        self.builders
            .iter()
            .any(|b| b.build(false).complexity() > 1 || b.merged_sarg)
            || (self.new_terms_count == 1
                && self
                    .builders
                    .iter()
                    .all(|b| simple_sarg(&b.build(self.negate))))
    }

    fn fix(&self, builder: RexBuilder, term: &CollectedTerm, unknown_as: RexUnknownAs) -> RexNode {
        match term {
            CollectedTerm::Node(n) => n.clone(),
            CollectedTerm::Sarg(i) => {
                let b = &self.builders[*i];
                let sarg = b.build(self.negate);
                let ty = b.get_type();
                if sarg.complexity() <= 1 && simple_sarg(&sarg) {
                    return sarg_ref(builder, &b.ref_node, &sarg, &ty, unknown_as);
                }
                builder.make_search(b.ref_node.clone(), sarg, ty)
            }
        }
    }
}

fn simple_sarg(sarg: &Sarg) -> bool {
    sarg.is_points() || sarg.range_set.is_open_interval() || sarg.is_complemented_points()
}

fn make_sarg_literal(v: &SargValue, ty: &RelDataType) -> RexNode {
    RexBuilder::make_literal(LiteralValue::from_sarg_value(v), ty.with_nullability(false))
}

pub fn sarg_ref(
    builder: RexBuilder,
    ref_node: &RexNode,
    sarg: &Sarg,
    ty: &RelDataType,
    unknown_as: RexUnknownAs,
) -> RexNode {
    if sarg.is_all() || sarg.is_none() {
        return simple_sarg_node(builder, ref_node, sarg, unknown_as);
    }
    let mut or_list = Vec::new();
    if sarg.null_as == RexUnknownAs::True && unknown_as == RexUnknownAs::Unknown {
        or_list.push(builder.make_call(&operators::IS_NULL, vec![ref_node.clone()]));
    }
    if sarg.is_points() {
        for r in sarg.range_set.as_ranges() {
            if let Some(v) = r.lower.endpoint() {
                or_list.push(builder.make_call(
                    &operators::EQUALS,
                    vec![ref_node.clone(), make_sarg_literal(v, ty)],
                ));
            }
        }
    } else if sarg.is_complemented_points() {
        let list: Vec<RexNode> = sarg
            .range_set
            .complement()
            .as_ranges()
            .iter()
            .filter_map(|r| r.lower.endpoint().cloned())
            .map(|v| {
                builder.make_call(
                    &operators::NOT_EQUALS,
                    vec![ref_node.clone(), make_sarg_literal(&v, ty)],
                )
            })
            .collect();
        or_list.push(compose_conjunction(builder, list));
    } else {
        for r in sarg.range_set.as_ranges() {
            or_list.push(range_to_rex(builder, ref_node, r, ty));
        }
    }
    let mut node = compose_disjunction(builder, or_list);
    if sarg.null_as == RexUnknownAs::False && unknown_as == RexUnknownAs::Unknown {
        node = builder.make_call(
            &operators::AND,
            vec![
                builder.make_call(&operators::IS_NOT_NULL, vec![ref_node.clone()]),
                node,
            ],
        );
    }
    node
}

fn range_to_rex(builder: RexBuilder, ref_node: &RexNode, r: &Range, ty: &RelDataType) -> RexNode {
    use super::sarg::Cut;
    let op = |o: &'static SqlOperator, v: &SargValue| {
        builder.make_call(o, vec![ref_node.clone(), make_sarg_literal(v, ty)])
    };
    match (&r.lower, &r.upper) {
        (Cut::BelowAll, Cut::AboveAll) => builder.make_bool_literal(true),
        (Cut::BelowAll, Cut::Below(u)) => op(&operators::LESS_THAN, u),
        (Cut::BelowAll, Cut::Above(u)) => op(&operators::LESS_THAN_OR_EQUAL, u),
        (Cut::Below(l), Cut::AboveAll) => op(&operators::GREATER_THAN_OR_EQUAL, l),
        (Cut::Above(l), Cut::AboveAll) => op(&operators::GREATER_THAN, l),
        (Cut::Below(l), Cut::Above(u)) if l.compare(u) == Ordering::Equal => {
            op(&operators::EQUALS, l)
        }
        (lower, upper) => {
            let lo = match lower {
                Cut::Below(l) => op(&operators::GREATER_THAN_OR_EQUAL, l),
                Cut::Above(l) => op(&operators::GREATER_THAN, l),
                _ => builder.make_bool_literal(true),
            };
            let hi = match upper {
                Cut::Above(u) => op(&operators::LESS_THAN_OR_EQUAL, u),
                Cut::Below(u) => op(&operators::LESS_THAN, u),
                _ => builder.make_bool_literal(true),
            };
            builder.make_call(&operators::AND, vec![lo, hi])
        }
    }
}

pub fn simple_sarg_node(
    builder: RexBuilder,
    ref_node: &RexNode,
    sarg: &Sarg,
    unknown_as: RexUnknownAs,
) -> RexNode {
    let null_as = if sarg.null_as == RexUnknownAs::Unknown {
        unknown_as
    } else {
        sarg.null_as
    };
    if sarg.is_all() {
        return match null_as {
            RexUnknownAs::True => builder.make_bool_literal(true),
            RexUnknownAs::False => {
                builder.make_call(&operators::IS_NOT_NULL, vec![ref_node.clone()])
            }
            RexUnknownAs::Unknown => builder.make_call(
                &operators::OR,
                vec![
                    builder.make_call(&operators::IS_NOT_NULL, vec![ref_node.clone()]),
                    RexBuilder::make_null_literal(&builder.boolean_type(true)),
                ],
            ),
        };
    }
    match null_as {
        RexUnknownAs::True => builder.make_call(&operators::IS_NULL, vec![ref_node.clone()]),
        RexUnknownAs::False => builder.make_bool_literal(false),
        RexUnknownAs::Unknown => builder.make_call(
            &operators::NOT_EQUALS,
            vec![ref_node.clone(), ref_node.clone()],
        ),
    }
}

pub fn expand_search(builder: RexBuilder, call: &RexCall) -> RexNode {
    match call.operands.get(1) {
        Some(RexNode::Literal(RexLiteral {
            value: LiteralValue::Sarg(sarg),
            ty,
        })) => sarg_ref(builder, &call.operands[0], sarg, ty, RexUnknownAs::Unknown),
        _ => RexNode::Call(call.clone()),
    }
}

fn rex_not(builder: RexBuilder, e: &RexNode) -> RexNode {
    match e {
        RexNode::Call(c) if c.op.kind == SqlKind::Not => c.operands[0].clone(),
        _ => builder.make_not(e.clone()),
    }
}

struct Comparison {
    ref_node: RexNode,
    kind: SqlKind,
    literal: RexLiteral,
}

fn comparison_of(e: &RexNode) -> Option<Comparison> {
    let call = e.as_call()?;
    if !call.op.kind.is_binary_comparison() {
        return None;
    }
    let left = &call.operands[0];
    let right = &call.operands[1];
    if let RexNode::Literal(lit) = right {
        return Some(Comparison {
            ref_node: left.clone(),
            kind: call.op.kind,
            literal: lit.clone(),
        });
    }
    if let RexNode::Literal(lit) = left {
        return Some(Comparison {
            ref_node: right.clone(),
            kind: call.op.kind.reverse(),
            literal: lit.clone(),
        });
    }
    None
}

fn is_upper_bound(e: &RexNode) -> bool {
    let Some(c) = e.as_call() else { return false };
    match c.op.kind {
        SqlKind::LessThan | SqlKind::LessThanOrEqual => {
            matches!(c.operands[0], RexNode::InputRef { .. })
                && matches!(c.operands[1], RexNode::Literal(_))
        }
        SqlKind::GreaterThan | SqlKind::GreaterThanOrEqual => {
            matches!(c.operands[1], RexNode::InputRef { .. })
                && matches!(c.operands[0], RexNode::Literal(_))
        }
        _ => false,
    }
}

fn is_lower_bound(e: &RexNode) -> bool {
    let Some(c) = e.as_call() else { return false };
    match c.op.kind {
        SqlKind::LessThan | SqlKind::LessThanOrEqual => {
            matches!(c.operands[1], RexNode::InputRef { .. })
                && matches!(c.operands[0], RexNode::Literal(_))
        }
        SqlKind::GreaterThan | SqlKind::GreaterThanOrEqual => {
            matches!(c.operands[0], RexNode::InputRef { .. })
                && matches!(c.operands[1], RexNode::Literal(_))
        }
        _ => false,
    }
}

fn replace_last(terms: &mut [RexNode], old: &RexNode, new: &RexNode) {
    if let Some(i) = terms.iter().rposition(|t| t == old) {
        terms[i] = new.clone();
    }
}

#[derive(Clone)]
struct SimpleRange {
    lower: Option<(SargValue, bool)>,
    upper: Option<(SargValue, bool)>,
}

impl SimpleRange {
    fn of(kind: SqlKind, v: SargValue) -> SimpleRange {
        match kind {
            SqlKind::Equals => SimpleRange {
                lower: Some((v.clone(), true)),
                upper: Some((v, true)),
            },
            SqlKind::LessThan => SimpleRange {
                lower: None,
                upper: Some((v, false)),
            },
            SqlKind::LessThanOrEqual => SimpleRange {
                lower: None,
                upper: Some((v, true)),
            },
            SqlKind::GreaterThan => SimpleRange {
                lower: Some((v, false)),
                upper: None,
            },
            _ => SimpleRange {
                lower: Some((v, true)),
                upper: None,
            },
        }
    }

    fn contains(&self, v: &SargValue) -> bool {
        if let Some((l, closed)) = &self.lower {
            match v.compare(l) {
                Ordering::Less => return false,
                Ordering::Equal if !closed => return false,
                _ => {}
            }
        }
        if let Some((u, closed)) = &self.upper {
            match v.compare(u) {
                Ordering::Greater => return false,
                Ordering::Equal if !closed => return false,
                _ => {}
            }
        }
        true
    }

    fn is_empty(&self) -> bool {
        match (&self.lower, &self.upper) {
            (Some((l, lc)), Some((u, uc))) => match l.compare(u) {
                Ordering::Greater => true,
                Ordering::Equal => !(*lc && *uc),
                Ordering::Less => false,
            },
            _ => false,
        }
    }
}

struct RangeTerm {
    ref_node: RexNode,
    range: SimpleRange,
    terms: Vec<RexNode>,
}

#[derive(Default)]
struct AndState {
    range_terms: Vec<RangeTerm>,
    equality_constants: Vec<(RexNode, RexLiteral)>,
    equality_terms: Vec<(RexNode, RexNode, RexNode)>,
    negated_terms: Vec<RexNode>,
}

fn strong_operands(terms: &[RexNode]) -> Vec<usize> {
    let mut strong: Vec<usize> = Vec::new();
    for t in terms {
        let mut refs = Vec::new();
        t.collect_input_refs(&mut refs);
        for r in refs {
            if is_not_true_when_null(t, r) && !strong.contains(&r) {
                strong.push(r);
            }
        }
    }
    strong
}

fn strip_true_equals(slot: &mut RexNode) -> RexNode {
    let mut term = slot.clone();
    while term.kind() == SqlKind::Equals {
        let Some(c) = term.as_call().cloned() else {
            break;
        };
        if c.operands[0].is_always_true() {
            term = c.operands[1].clone();
        } else if c.operands[1].is_always_true() {
            term = c.operands[0].clone();
        } else {
            break;
        }
        *slot = term.clone();
    }
    term
}

enum BoundChange {
    Empty,
    Tighter,
    Looser,
}

fn beyond_bound(o: Ordering, beyond: Ordering, closed: bool) -> bool {
    if closed {
        o == beyond
    } else {
        o != beyond.reverse()
    }
}

fn tighten_bound(r: &mut SimpleRange, v0: SargValue, upper: bool, closed: bool) -> BoundChange {
    let beyond = if upper {
        Ordering::Less
    } else {
        Ordering::Greater
    };
    let (own, other) = if upper {
        (&r.upper, &r.lower)
    } else {
        (&r.lower, &r.upper)
    };
    let tighter = own
        .as_ref()
        .is_none_or(|(b, _)| beyond_bound(v0.compare(b), beyond, closed));
    if !tighter {
        return BoundChange::Looser;
    }
    if let Some((b, _)) = other {
        if beyond_bound(v0.compare(b), beyond, closed) {
            return BoundChange::Empty;
        }
    }
    if upper {
        r.upper = Some((v0, closed));
    } else {
        r.lower = Some((v0, closed));
    }
    if r.is_empty() {
        BoundChange::Empty
    } else {
        BoundChange::Tighter
    }
}

fn process_range(
    builder: RexBuilder,
    terms: &mut [RexNode],
    range_terms: &mut Vec<RangeTerm>,
    term: &RexNode,
    ref_node: &RexNode,
    v0: SargValue,
    comparison: SqlKind,
) -> Option<RexNode> {
    let true_literal = builder.make_bool_literal(true);
    let Some(pos) = range_terms.iter().position(|r| &r.ref_node == ref_node) else {
        range_terms.push(RangeTerm {
            ref_node: ref_node.clone(),
            range: SimpleRange::of(comparison, v0),
            terms: vec![term.clone()],
        });
        return None;
    };
    let mut r = range_terms[pos].range.clone();
    let previous = range_terms[pos].terms.clone();
    let mut remove_upper = false;
    let mut remove_lower = false;
    match comparison {
        SqlKind::Equals => {
            if !r.contains(&v0) {
                return Some(builder.make_bool_literal(false));
            }
            range_terms[pos] = RangeTerm {
                ref_node: ref_node.clone(),
                range: SimpleRange::of(SqlKind::Equals, v0),
                terms: vec![term.clone()],
            };
            for e in &previous {
                replace_last(terms, e, &true_literal);
            }
            return None;
        }
        SqlKind::LessThan | SqlKind::LessThanOrEqual => {
            let closed = comparison == SqlKind::LessThanOrEqual;
            match tighten_bound(&mut r, v0, true, closed) {
                BoundChange::Empty => return Some(builder.make_bool_literal(false)),
                BoundChange::Tighter => remove_upper = true,
                BoundChange::Looser => replace_last(terms, term, &true_literal),
            }
        }
        _ => {
            let closed = comparison == SqlKind::GreaterThanOrEqual;
            match tighten_bound(&mut r, v0, false, closed) {
                BoundChange::Empty => return Some(builder.make_bool_literal(false)),
                BoundChange::Tighter => remove_lower = true,
                BoundChange::Looser => replace_last(terms, term, &true_literal),
            }
        }
    }
    if remove_upper || remove_lower {
        let mut new_bounds = Vec::new();
        for e in &previous {
            let drop = if remove_upper {
                is_upper_bound(e)
            } else {
                is_lower_bound(e)
            };
            if drop {
                replace_last(terms, e, &true_literal);
            } else {
                new_bounds.push(e.clone());
            }
        }
        new_bounds.push(term.clone());
        range_terms[pos] = RangeTerm {
            ref_node: ref_node.clone(),
            range: r,
            terms: new_bounds,
        };
    }
    None
}

fn is_not_true_when_null(term: &RexNode, idx: usize) -> bool {
    is_null_when(term, idx)
        || match term {
            RexNode::Call(c) => match c.op.kind {
                SqlKind::And => c.operands.iter().any(|o| is_not_true_when_null(o, idx)),
                SqlKind::Or => c.operands.iter().all(|o| is_not_true_when_null(o, idx)),
                SqlKind::IsTrue | SqlKind::IsNotFalse => is_not_true_when_null(&c.operands[0], idx),
                _ => false,
            },
            _ => false,
        }
}

fn is_strong_kind(kind: SqlKind) -> bool {
    matches!(
        kind,
        SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThan
            | SqlKind::LessThanOrEqual
            | SqlKind::Plus
            | SqlKind::Minus
            | SqlKind::Times
            | SqlKind::Divide
            | SqlKind::Mod
            | SqlKind::MinusPrefix
            | SqlKind::PlusPrefix
            | SqlKind::Like
            | SqlKind::Cast
            | SqlKind::Search
            | SqlKind::Not
            | SqlKind::Extract
            | SqlKind::Ceil
            | SqlKind::Floor
            | SqlKind::CharLength
    )
}

fn is_null_when(e: &RexNode, idx: usize) -> bool {
    match e {
        RexNode::InputRef { index, .. } => *index == idx,
        RexNode::Literal(l) => l.is_null(),
        RexNode::Call(c) if is_strong_kind(c.op.kind) => {
            c.operands.iter().any(|o| is_null_when(o, idx))
        }
        RexNode::Call(c) if c.op.kind == SqlKind::And => {
            c.operands.iter().any(|o| is_null_when(o, idx))
        }
        _ => false,
    }
}

fn definitely_null(e: &RexNode) -> bool {
    match e {
        RexNode::Literal(l) => l.is_null(),
        RexNode::Call(c) if is_strong_kind(c.op.kind) && c.op.kind != SqlKind::Search => {
            c.operands.iter().any(definitely_null)
        }
        _ => false,
    }
}

fn literal_bool(e: &RexNode) -> Option<bool> {
    match e {
        RexNode::Literal(RexLiteral {
            value: LiteralValue::Boolean(b),
            ..
        }) => Some(*b),
        _ => None,
    }
}

impl RexSimplify {
    pub fn new(builder: RexBuilder) -> RexSimplify {
        RexSimplify {
            builder,
            predicate_elimination: true,
        }
    }

    fn with_predicate_elimination(self, on: bool) -> RexSimplify {
        RexSimplify {
            builder: self.builder,
            predicate_elimination: on,
        }
    }

    pub fn simplify_filter_predicates(self, predicates: Vec<RexNode>) -> Option<RexNode> {
        let s = self.with_predicate_elimination(false);
        let composed = compose_conjunction(self.builder, predicates);
        let simplified = s.simplify(&composed, RexUnknownAs::False);
        if simplified.is_always_false() {
            return None;
        }
        Some(remove_nullability_cast(&simplified))
    }

    pub fn simplify_unknown_as_false(self, e: &RexNode) -> RexNode {
        self.simplify(e, RexUnknownAs::False)
    }

    pub fn simplify_preserving_type(self, e: &RexNode) -> RexNode {
        let e2 = self.simplify(e, RexUnknownAs::Unknown);
        if e2.get_type() == e.get_type() {
            return e2;
        }
        let e3 = RexBuilder::make_cast(e.get_type().clone(), e2, false);
        if &e3 == e {
            return e.clone();
        }
        e3
    }

    pub fn simplify(self, e: &RexNode, unknown_as: RexUnknownAs) -> RexNode {
        if definitely_null(e) {
            if e.get_type().sql_type_name() == SqlTypeName::Boolean {
                if let Some(b) = unknown_as.to_boolean() {
                    return self.builder.make_bool_literal(b);
                }
            }
            if e.is_null_literal() {
                return e.clone();
            }
            return RexBuilder::make_null_literal(e.get_type());
        }
        let RexNode::Call(call) = e else {
            return e.clone();
        };
        match call.op.kind {
            SqlKind::And => self.simplify_and(e, unknown_as),
            SqlKind::Or => self.simplify_or(e, unknown_as),
            SqlKind::Not => self.simplify_not(call, unknown_as),
            SqlKind::Case => self.simplify_case(call, unknown_as),
            SqlKind::Coalesce => self.simplify_coalesce(call),
            SqlKind::Cast | SqlKind::SafeCast => self.simplify_cast(call),
            SqlKind::IsNull
            | SqlKind::IsNotNull
            | SqlKind::IsTrue
            | SqlKind::IsNotTrue
            | SqlKind::IsFalse
            | SqlKind::IsNotFalse => self.simplify_is(call, unknown_as),
            SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThan
            | SqlKind::LessThanOrEqual => self.simplify_comparison(call, unknown_as),
            SqlKind::Search => self.simplify_search(call, unknown_as),
            SqlKind::Like if !call.op.negated => self.simplify_like(call, unknown_as),
            SqlKind::MinusPrefix => {
                if call.operands[0].kind() == SqlKind::MinusPrefix {
                    return self.simplify(&call.operands[0].operands()[0], unknown_as);
                }
                self.simplify_generic(call)
            }
            SqlKind::PlusPrefix => self.simplify(&call.operands[0], unknown_as),
            SqlKind::Plus | SqlKind::Minus | SqlKind::Times | SqlKind::Divide => {
                self.simplify_arithmetic(call)
            }
            _ => self.simplify_generic(call),
        }
    }

    fn simplify_list(self, terms: &mut [RexNode], unknown_as: RexUnknownAs) {
        for t in terms.iter_mut() {
            *t = self.simplify(t, unknown_as);
        }
    }

    fn simplify_generic(self, e: &RexCall) -> RexNode {
        let mut operands = e.operands.clone();
        self.simplify_list(&mut operands, RexUnknownAs::Unknown);
        if operands == e.operands {
            return RexNode::Call(e.clone());
        }
        RexBuilder::make_call_typed(e.ty.clone(), e.op, operands)
    }

    fn find_literal_index(operands: &[RexNode], value: i64) -> Option<usize> {
        let target = BigDecimal::from(value);
        operands.iter().position(|o| {
            matches!(o, RexNode::Literal(RexLiteral { value: LiteralValue::Exact(d), .. }) if *d == target)
        })
    }

    fn simplify_arithmetic(self, e: &RexCall) -> RexNode {
        if e.ty.sql_type_name().family() != SqlTypeFamily::Numeric || e.operands.len() != 2 {
            return self.simplify_generic(e);
        }
        let keep = |other: &RexNode| -> RexNode {
            if other.get_type() == &e.ty {
                other.clone()
            } else {
                RexBuilder::make_cast(e.ty.clone(), other.clone(), false)
            }
        };
        match e.op.kind {
            SqlKind::Plus => {
                if let Some(i) = Self::find_literal_index(&e.operands, 0) {
                    return keep(&e.operands[(i + 1) % 2]);
                }
            }
            SqlKind::Minus => {
                if Self::find_literal_index(&e.operands, 0) == Some(1) {
                    return keep(&e.operands[0]);
                }
            }
            SqlKind::Times => {
                if let Some(i) = Self::find_literal_index(&e.operands, 1) {
                    return keep(&e.operands[(i + 1) % 2]);
                }
            }
            SqlKind::Divide if Self::find_literal_index(&e.operands, 1) == Some(1) => {
                return keep(&e.operands[0]);
            }
            _ => {}
        }
        self.simplify_generic(e)
    }

    fn simplify_like(self, e: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        if let Some(RexNode::Literal(RexLiteral {
            value: LiteralValue::Char(s),
            ..
        })) = e.operands.get(1)
        {
            if s == "%" && e.operands.len() == 2 {
                let x = e.operands[0].clone();
                let eq = self
                    .builder
                    .make_call(&operators::EQUALS, vec![x.clone(), x]);
                return self.simplify(&eq, unknown_as);
            }
        }
        self.simplify_generic(e)
    }

    fn simplify_comparison(self, e: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let mut operands = e.operands.clone();
        self.simplify_list(&mut operands, RexUnknownAs::Unknown);
        let o0 = operands[0].clone();
        let o1 = operands[1].clone();
        if o0 == o1 {
            match e.op.kind {
                SqlKind::Equals | SqlKind::GreaterThanOrEqual | SqlKind::LessThanOrEqual => {
                    let n = self.builder.make_call(
                        &operators::OR,
                        vec![
                            RexBuilder::make_null_literal(&e.ty),
                            self.builder
                                .make_call(&operators::IS_NOT_NULL, vec![o0.clone()]),
                        ],
                    );
                    return self.simplify(&n, unknown_as);
                }
                _ => {
                    let n = self.builder.make_call(
                        &operators::AND,
                        vec![
                            RexBuilder::make_null_literal(&e.ty),
                            self.builder
                                .make_call(&operators::IS_NULL, vec![o0.clone()]),
                        ],
                    );
                    return self.simplify(&n, unknown_as);
                }
            }
        }
        if o0.get_type().sql_type_name() == SqlTypeName::Boolean {
            let rebuilt = self.builder.make_call(e.op, vec![o0.clone(), o1.clone()]);
            if let Some(cmp) = comparison_of(&rebuilt) {
                if let LiteralValue::Boolean(b) = cmp.literal.value {
                    let nullable = cmp.ref_node.get_type().is_nullable();
                    if b {
                        match cmp.kind {
                            SqlKind::GreaterThanOrEqual | SqlKind::Equals => return cmp.ref_node,
                            SqlKind::LessThan | SqlKind::NotEquals => {
                                return self
                                    .simplify(&rex_not(self.builder, &cmp.ref_node), unknown_as)
                            }
                            SqlKind::GreaterThan if !nullable => {
                                return self.builder.make_bool_literal(false)
                            }
                            SqlKind::LessThanOrEqual if !nullable => {
                                return self.builder.make_bool_literal(true)
                            }
                            _ => {}
                        }
                    } else {
                        match cmp.kind {
                            SqlKind::Equals | SqlKind::LessThanOrEqual => {
                                return self
                                    .simplify(&rex_not(self.builder, &cmp.ref_node), unknown_as)
                            }
                            SqlKind::NotEquals | SqlKind::GreaterThan => return cmp.ref_node,
                            SqlKind::GreaterThanOrEqual if !nullable => {
                                return self.builder.make_bool_literal(true)
                            }
                            SqlKind::LessThan if !nullable => {
                                return self.builder.make_bool_literal(false)
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if let (RexNode::Literal(l0), RexNode::Literal(l1)) = (&o0, &o1) {
            if l0.ty.equals_sans_nullability(&l1.ty) {
                if l0.is_null() || l1.is_null() {
                    return if unknown_as == RexUnknownAs::False {
                        self.builder.make_bool_literal(false)
                    } else {
                        RexBuilder::make_null_literal(&e.ty)
                    };
                }
                if let (Some(v0), Some(v1)) = (l0.value.to_sarg_value(), l1.value.to_sarg_value()) {
                    let c = v0.compare(&v1);
                    let r = match e.op.kind {
                        SqlKind::Equals => c == Ordering::Equal,
                        SqlKind::GreaterThan => c == Ordering::Greater,
                        SqlKind::GreaterThanOrEqual => c != Ordering::Less,
                        SqlKind::LessThan => c == Ordering::Less,
                        SqlKind::LessThanOrEqual => c != Ordering::Greater,
                        _ => c != Ordering::Equal,
                    };
                    return self.builder.make_bool_literal(r);
                }
            }
        }
        if operands == e.operands {
            RexNode::Call(e.clone())
        } else {
            self.builder.make_call(e.op, operands)
        }
    }

    fn simplify_and(self, e: &RexNode, unknown_as: RexUnknownAs) -> RexNode {
        let mut operands = conjunctions(e);
        if unknown_as == RexUnknownAs::False && self.predicate_elimination {
            self.simplify_and_terms(&mut operands);
        } else {
            self.simplify_list(&mut operands, unknown_as);
        }
        let mut collected = Vec::new();
        let mut collector = SargCollector::new(true);
        for t in &operands {
            collector.accept(t, &mut collected);
        }
        if collector.need_to_fix() {
            operands = collected
                .iter()
                .map(|t| collector.fix(self.builder, t, unknown_as))
                .collect();
        }
        let mut terms = Vec::new();
        let mut not_terms = Vec::new();
        for o in &operands {
            decompose_conjunction(o, &mut terms, &mut not_terms);
        }
        if unknown_as == RexUnknownAs::False {
            return self.simplify_and2_for_unknown_as_false(terms, &not_terms);
        }
        self.simplify_and2(terms, not_terms)
    }

    fn simplify_and_terms(self, terms: &mut [RexNode]) {
        let is_predicate = |t: &RexNode| {
            comparison_of(t).is_some() || matches!(t.kind(), SqlKind::IsNull | SqlKind::IsNotNull)
        };
        for t in terms.iter_mut() {
            if is_predicate(t) {
                *t = self.simplify(t, RexUnknownAs::False);
            }
        }
        for t in terms.iter_mut() {
            if !is_predicate(t) {
                *t = self.simplify(t, RexUnknownAs::False);
            }
        }
    }

    fn simplify_and2(self, mut terms: Vec<RexNode>, mut not_terms: Vec<RexNode>) -> RexNode {
        if terms.iter().any(RexNode::is_always_false) {
            return self.builder.make_bool_literal(false);
        }
        if terms.is_empty() && not_terms.is_empty() {
            return self.builder.make_bool_literal(true);
        }
        let mut not_satisfiable: Vec<RexNode> = Vec::new();
        for nd in &not_terms {
            let terms2 = conjunctions(nd);
            if !terms2.iter().all(|t| terms.contains(t)) {
                continue;
            }
            if !nd.get_type().is_nullable() {
                return self.builder.make_bool_literal(false);
            }
            not_satisfiable.push(nd.clone());
        }
        if let Some(first) = not_satisfiable.first().cloned() {
            terms.retain(|t| !not_satisfiable.contains(t));
            not_terms.retain(|t| !not_satisfiable.contains(t));
            terms.push(RexBuilder::make_null_literal(first.get_type()));
            for ns in &not_satisfiable {
                let is_null = self
                    .builder
                    .make_call(&operators::IS_NULL, vec![ns.clone()]);
                if let RexNode::Call(c) = &is_null {
                    terms.push(self.simplify_is(c, RexUnknownAs::Unknown));
                }
            }
        }
        for nd in &not_terms {
            terms.push(self.simplify(&rex_not(self.builder, nd), RexUnknownAs::Unknown));
        }
        compose_conjunction(self.builder, terms)
    }

    fn simplify_and2_for_unknown_as_false(
        self,
        mut terms: Vec<RexNode>,
        not_terms: &[RexNode],
    ) -> RexNode {
        if terms
            .iter()
            .any(|t| t.is_always_false() || t.is_null_literal())
        {
            return self.builder.make_bool_literal(false);
        }
        if terms.is_empty() && not_terms.is_empty() {
            return self.builder.make_bool_literal(true);
        }
        if terms.len() == 1 && not_terms.is_empty() {
            return self.simplify(&terms[0], RexUnknownAs::False);
        }
        let mut state = AndState::default();
        let mut null_operands: Vec<RexNode> = Vec::new();
        let mut not_null_operands: Vec<RexNode> = Vec::new();
        let mut i = 0;
        while i < terms.len() {
            let term = strip_true_equals(&mut terms[i]);
            match term.kind() {
                k if k.is_binary_comparison() => {
                    if let Some(r) = self.absorb_comparison(k, &term, &mut terms, &mut state) {
                        return r;
                    }
                }
                SqlKind::IsNotNull => {
                    not_null_operands.push(term.operands()[0].clone());
                    terms.remove(i);
                    continue;
                }
                SqlKind::IsNull => {
                    null_operands.push(term.operands()[0].clone());
                }
                _ => {}
            }
            i += 1;
        }
        for (ref1, ref2, eq_term) in &state.equality_terms {
            let l1 = state
                .equality_constants
                .iter()
                .find(|(r, _)| r == ref1)
                .map(|(_, l)| l.clone());
            let Some(l1) = l1 else { continue };
            let l2 = state
                .equality_constants
                .iter()
                .find(|(r, _)| r == ref2)
                .map(|(_, l)| l.clone());
            let Some(l2) = l2 else { continue };
            if l1.ty == l2.ty && l1 != l2 {
                return self.builder.make_bool_literal(false);
            }
            if let Some(p) = terms.iter().position(|t| t == eq_term) {
                terms.remove(p);
            }
        }
        for nd in not_terms {
            let terms2 = conjunctions(nd);
            if terms2.iter().all(|t| terms.contains(t)) {
                return self.builder.make_bool_literal(false);
            }
        }
        for nt in &state.negated_terms {
            if terms.contains(nt) {
                return self.builder.make_bool_literal(false);
            }
        }
        for nd in not_terms {
            terms.push(rex_not(self.builder, nd));
        }
        let strong_operands = strong_operands(&terms);
        if null_operands.iter().any(
            |n| matches!(n, RexNode::InputRef { index, .. } if strong_operands.contains(index)),
        ) {
            return self.builder.make_bool_literal(false);
        }
        let mut seen: Vec<RexNode> = Vec::new();
        for operand in not_null_operands {
            if seen.contains(&operand) {
                continue;
            }
            seen.push(operand.clone());
            let strong = matches!(&operand, RexNode::InputRef { index, .. } if strong_operands.contains(index));
            if !strong {
                terms.push(
                    self.builder
                        .make_call(&operators::IS_NOT_NULL, vec![operand]),
                );
            }
        }
        compose_conjunction(self.builder, terms)
    }

    fn absorb_comparison(
        self,
        k: SqlKind,
        term: &RexNode,
        terms: &mut [RexNode],
        state: &mut AndState,
    ) -> Option<RexNode> {
        let call = term.as_call()?;
        let comparison = comparison_of(term);
        if let Some(cmp) = &comparison {
            if cmp.literal.is_null() {
                return Some(self.builder.make_bool_literal(false));
            }
        }
        if k == SqlKind::Equals {
            if let Some(cmp) = &comparison {
                if let Some(pos) = state
                    .equality_constants
                    .iter()
                    .position(|(r, _)| r == &cmp.ref_node)
                {
                    let prev = state.equality_constants[pos].1.clone();
                    state.equality_constants[pos].1 = cmp.literal.clone();
                    if cmp.literal.ty == prev.ty && cmp.literal != prev {
                        return Some(self.builder.make_bool_literal(false));
                    }
                } else {
                    state
                        .equality_constants
                        .push((cmp.ref_node.clone(), cmp.literal.clone()));
                }
            } else if matches!(call.operands[0], RexNode::InputRef { .. })
                && matches!(call.operands[1], RexNode::InputRef { .. })
            {
                state.equality_terms.push((
                    call.operands[0].clone(),
                    call.operands[1].clone(),
                    term.clone(),
                ));
            }
        }
        let negated_kind = k.negate_null_safe();
        if let Some(op) = operators::of_comparison_kind(negated_kind) {
            let neg = self.builder.make_call(op, call.operands.clone());
            state.negated_terms.push(neg);
            if let Some(inv) = operators::of_comparison_kind(negated_kind.reverse()) {
                let mut ops = call.operands.clone();
                ops.reverse();
                state.negated_terms.push(self.builder.make_call(inv, ops));
            }
        }
        if let Some(cmp) = comparison {
            if cmp.kind != SqlKind::NotEquals {
                if let Some(v) = cmp.literal.value.to_sarg_value() {
                    if let Some(r) = process_range(
                        self.builder,
                        terms,
                        &mut state.range_terms,
                        term,
                        &cmp.ref_node,
                        v,
                        cmp.kind,
                    ) {
                        return Some(r);
                    }
                }
            }
        }
        None
    }

    fn simplify_or(self, e: &RexNode, unknown_as: RexUnknownAs) -> RexNode {
        let terms0 = disjunctions(e);
        let mut terms = if self.predicate_elimination {
            let (mut nulls, rest): (Vec<RexNode>, Vec<RexNode>) = terms0
                .into_iter()
                .partition(|t| t.kind() == SqlKind::IsNull);
            nulls.extend(rest);
            nulls
        } else {
            terms0
        };
        self.simplify_list(&mut terms, unknown_as);
        self.simplify_ors(terms, unknown_as)
    }

    fn simplify_ors(self, mut terms: Vec<RexNode>, unknown_as: RexUnknownAs) -> RexNode {
        let mut collected = Vec::new();
        let mut collector = SargCollector::new(false);
        for t in &terms {
            collector.accept(t, &mut collected);
        }
        if collector.need_to_fix() {
            terms = collected
                .iter()
                .map(|t| collector.fix(self.builder, t, unknown_as))
                .collect();
        }
        let true_literal = self.builder.make_bool_literal(true);
        let mut not_equals: Vec<(RexNode, RexNode)> = Vec::new();
        let mut i = 0;
        while i < terms.len() {
            let term = terms[i].clone();
            match term.kind() {
                SqlKind::Literal => {
                    if term.is_null_literal() {
                        if unknown_as == RexUnknownAs::False {
                            terms.remove(i);
                            continue;
                        } else if unknown_as == RexUnknownAs::True {
                            return true_literal;
                        }
                    } else if literal_bool(&term) == Some(true) {
                        return term;
                    } else {
                        terms.remove(i);
                        continue;
                    }
                }
                SqlKind::NotEquals => {
                    if let Some(cmp) = comparison_of(&term) {
                        if let Some(pos) = not_equals.iter().position(|(r, _)| r == &cmp.ref_node) {
                            let prev = not_equals[pos].1.clone();
                            if prev.kind() != SqlKind::IsNotNull {
                                let v1 = cmp.literal.value.to_sarg_value();
                                let v2 = comparison_of(&prev)
                                    .and_then(|c| c.literal.value.to_sarg_value());
                                if let (Some(v1), Some(v2)) = (v1, v2) {
                                    if v1.compare(&v2) != Ordering::Equal {
                                        let is_not_null = self.builder.make_call(
                                            &operators::IS_NOT_NULL,
                                            vec![cmp.ref_node.clone()],
                                        );
                                        let cond = self.simplify(
                                            &self.builder.make_call(
                                                &operators::OR,
                                                vec![
                                                    is_not_null.clone(),
                                                    RexBuilder::make_null_literal(
                                                        &self.builder.boolean_type(true),
                                                    ),
                                                ],
                                            ),
                                            unknown_as,
                                        );
                                        if cond.is_always_true() {
                                            return true_literal;
                                        }
                                        not_equals[pos].1 = is_not_null;
                                        if let Some(p) = terms.iter().position(|t| t == &prev) {
                                            terms[p] = cond;
                                        }
                                    }
                                }
                            }
                            terms.remove(i);
                            continue;
                        }
                        not_equals.push((cmp.ref_node.clone(), term.clone()));
                    }
                }
                SqlKind::Not => {
                    let x = term.operands()[0].clone();
                    if terms.contains(&x) {
                        if !x.get_type().is_nullable() {
                            return true_literal;
                        }
                        let is_not_null = self
                            .builder
                            .make_call(&operators::IS_NOT_NULL, vec![x.clone()]);
                        let pos = terms.iter().position(|t| t == &x);
                        if let (Some(p), RexNode::Call(c)) = (pos, &is_not_null) {
                            terms[p] = self.simplify_is(c, unknown_as);
                        }
                        terms[i] = RexBuilder::make_null_literal(x.get_type());
                        continue;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        compose_disjunction(self.builder, terms)
    }

    fn simplify_not(self, call: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let a = &call.operands[0];
        match a {
            RexNode::Call(ac) => match ac.op.kind {
                SqlKind::Not => return self.simplify(&ac.operands[0], unknown_as),
                SqlKind::Search => {
                    if let Some(RexNode::Literal(RexLiteral {
                        value: LiteralValue::Sarg(sarg),
                        ty,
                    })) = ac.operands.get(1)
                    {
                        let negated = RexCall {
                            op: ac.op,
                            operands: vec![
                                ac.operands[0].clone(),
                                RexNode::Literal(RexLiteral {
                                    value: LiteralValue::Sarg(Box::new(sarg.negate())),
                                    ty: ty.clone(),
                                }),
                            ],
                            ty: ac.ty.clone(),
                        };
                        return self.simplify_search(&negated, unknown_as.negate());
                    }
                }
                SqlKind::And | SqlKind::Or => {
                    let ops: Vec<RexNode> = ac
                        .operands
                        .iter()
                        .map(|o| self.simplify(&rex_not(self.builder, o), unknown_as))
                        .collect();
                    let op = if ac.op.kind == SqlKind::And {
                        &operators::OR
                    } else {
                        &operators::AND
                    };
                    return self.simplify(&self.builder.make_call(op, ops), unknown_as);
                }
                SqlKind::Case => {
                    let mut ops = Vec::new();
                    let n = ac.operands.len();
                    let mut i = 0;
                    while i < n {
                        if i + 1 == n {
                            ops.push(rex_not(self.builder, &ac.operands[i]));
                        } else {
                            ops.push(ac.operands[i].clone());
                            ops.push(rex_not(self.builder, &ac.operands[i + 1]));
                        }
                        i += 2;
                    }
                    let c = RexBuilder::make_call_typed(ac.ty.clone(), &operators::CASE, ops);
                    return self.simplify(&c, unknown_as);
                }
                SqlKind::In | SqlKind::NotIn => {}
                k => {
                    let negated = match k {
                        SqlKind::IsTrue
                        | SqlKind::IsFalse
                        | SqlKind::IsNull
                        | SqlKind::IsNotTrue
                        | SqlKind::IsNotFalse
                        | SqlKind::IsNotNull
                        | SqlKind::IsDistinctFrom
                        | SqlKind::IsNotDistinctFrom => k.negate_null_safe(),
                        _ => k,
                    };
                    let negated = if negated != k {
                        negated
                    } else if k.is_binary_comparison() {
                        k.negate_null_safe()
                    } else {
                        k
                    };
                    if negated != k {
                        if let Some(op) = operators::of_comparison_kind(negated) {
                            let c = self.builder.make_call(op, ac.operands.clone());
                            return self.simplify(&c, unknown_as);
                        }
                    }
                }
            },
            RexNode::Literal(RexLiteral {
                value: LiteralValue::Boolean(b),
                ..
            }) => return self.builder.make_bool_literal(!b),
            _ => {}
        }
        let a2 = self.simplify(a, unknown_as.negate());
        if &a2 == a {
            return RexNode::Call(call.clone());
        }
        rex_not(self.builder, &a2)
    }

    fn simplify_is(self, call: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        match self.simplify_is1(call.op.kind, &call.operands[0], unknown_as) {
            Some(n) => n,
            None => RexNode::Call(call.clone()),
        }
    }

    fn simplify_is1(self, kind: SqlKind, a: &RexNode, unknown_as: RexUnknownAs) -> Option<RexNode> {
        if kind == SqlKind::IsTrue && unknown_as == RexUnknownAs::False {
            return Some(self.simplify(a, unknown_as));
        }
        if kind == SqlKind::IsFalse && unknown_as == RexUnknownAs::False {
            return Some(self.simplify(&rex_not(self.builder, a), unknown_as));
        }
        if kind == SqlKind::IsNotFalse && unknown_as == RexUnknownAs::True {
            return Some(self.simplify(a, unknown_as));
        }
        if kind == SqlKind::IsNotTrue && unknown_as == RexUnknownAs::True {
            return Some(self.simplify(&rex_not(self.builder, a), unknown_as));
        }
        self.simplify_is2(kind, a, unknown_as)
    }

    fn simplify_is2(self, kind: SqlKind, a: &RexNode, unknown_as: RexUnknownAs) -> Option<RexNode> {
        match kind {
            SqlKind::IsNull => {
                if let Some(s) = self.simplify_is_null(a) {
                    return Some(s);
                }
            }
            SqlKind::IsNotNull => {
                if let Some(s) = self.simplify_is_not_null(a) {
                    return Some(s);
                }
            }
            SqlKind::IsTrue => {
                if !a.get_type().is_nullable() {
                    return Some(self.simplify(a, unknown_as));
                }
                let s = self.simplify(a, RexUnknownAs::False);
                if &s == a {
                    return None;
                }
                return Some(self.builder.make_call(&operators::IS_TRUE, vec![s]));
            }
            SqlKind::IsNotFalse => {
                if !a.get_type().is_nullable() {
                    return Some(self.simplify(a, unknown_as));
                }
                let s = self.simplify(a, RexUnknownAs::True);
                if &s == a {
                    return None;
                }
                return Some(self.builder.make_call(&operators::IS_NOT_FALSE, vec![s]));
            }
            SqlKind::IsFalse | SqlKind::IsNotTrue if !a.get_type().is_nullable() => {
                return Some(self.simplify(&rex_not(self.builder, a), unknown_as));
            }
            _ => {}
        }
        if a.kind() == SqlKind::Not {
            if let Some(op) = operators::of_comparison_kind(kind.negate_null_safe()) {
                let c = self.builder.make_call(op, vec![a.operands()[0].clone()]);
                return Some(self.simplify(&c, RexUnknownAs::Unknown));
            }
        }
        let a2 = self.simplify(a, RexUnknownAs::Unknown);
        if &a2 != a {
            if let Some(op) = operators::of_comparison_kind(kind) {
                return Some(self.builder.make_call(op, vec![a2]));
            }
        }
        None
    }

    fn simplify_is_not_null(self, a: &RexNode) -> Option<RexNode> {
        let a = self.simplify(a, RexUnknownAs::Unknown);
        if !a.get_type().is_nullable() {
            return Some(self.builder.make_bool_literal(true));
        }
        match &a {
            RexNode::Literal(l) => Some(self.builder.make_bool_literal(!l.is_null())),
            RexNode::Call(c) if is_strong_kind(c.op.kind) && c.op.kind != SqlKind::Search => {
                let mut operands = Vec::new();
                for o in &c.operands {
                    match self.simplify_is_not_null(o) {
                        None => operands.push(
                            self.builder
                                .make_call(&operators::IS_NOT_NULL, vec![o.clone()]),
                        ),
                        Some(s) if s.is_always_false() => {
                            return Some(self.builder.make_bool_literal(false))
                        }
                        Some(s) => operands.push(s),
                    }
                }
                Some(compose_conjunction(self.builder, operands))
            }
            _ => None,
        }
    }

    fn simplify_is_null(self, a: &RexNode) -> Option<RexNode> {
        let a = self.simplify(a, RexUnknownAs::Unknown);
        if !a.get_type().is_nullable() {
            return Some(self.builder.make_bool_literal(false));
        }
        match &a {
            RexNode::Literal(l) if l.is_null() => Some(self.builder.make_bool_literal(true)),
            RexNode::Literal(_) => Some(self.builder.make_bool_literal(false)),
            RexNode::Call(c) if is_strong_kind(c.op.kind) && c.op.kind != SqlKind::Search => {
                let mut operands = Vec::new();
                for o in &c.operands {
                    match self.simplify_is_null(o) {
                        None => operands
                            .push(self.builder.make_call(&operators::IS_NULL, vec![o.clone()])),
                        Some(s) => operands.push(s),
                    }
                }
                Some(compose_disjunction(self.builder, operands))
            }
            _ => None,
        }
    }

    fn simplify_search(self, call: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let a = &call.operands[0];
        if let Some(RexNode::Literal(RexLiteral {
            value: LiteralValue::Sarg(sarg),
            ty,
        })) = call.operands.get(1)
        {
            if sarg.is_all() || sarg.is_none() {
                let n = simple_sarg_node(self.builder, a, sarg, unknown_as);
                return self.simplify(&n, unknown_as);
            }
            if sarg.null_as != RexUnknownAs::Unknown {
                if let Some(s) = self.simplify_is1(SqlKind::IsNull, a, unknown_as) {
                    if s.is_always_false() {
                        let sarg2 = Sarg::of(RexUnknownAs::Unknown, sarg.range_set.clone());
                        let c2 = RexCall {
                            op: call.op,
                            operands: vec![
                                a.clone(),
                                RexNode::Literal(RexLiteral {
                                    value: LiteralValue::Sarg(Box::new(sarg2)),
                                    ty: ty.clone(),
                                }),
                            ],
                            ty: call.ty.clone(),
                        };
                        return self.simplify_search(&c2, unknown_as);
                    }
                }
            } else if sarg.is_points() && sarg.point_count() <= 1 {
                return expand_search(self.builder, call);
            }
        }
        RexNode::Call(call.clone())
    }

    fn simplify_cast(self, e: &RexCall) -> RexNode {
        let operand = self.simplify(&e.operands[0], RexUnknownAs::Unknown);
        let same_or_narrows = e.ty == *operand.get_type()
            || (e.ty.equals_sans_nullability(operand.get_type())
                && !operand.get_type().is_nullable());
        if same_or_narrows && e.ty.sql_type_name() != SqlTypeName::Decimal {
            return operand;
        }
        if let RexNode::Literal(lit) = &operand {
            if let Some(folded) = fold_literal_cast(lit, &e.ty) {
                return folded;
            }
        }
        if operand == e.operands[0] {
            RexNode::Call(e.clone())
        } else {
            RexBuilder::make_call_typed(e.ty.clone(), e.op, vec![operand])
        }
    }

    fn simplify_coalesce(self, call: &RexCall) -> RexNode {
        let mut operands: Vec<RexNode> = Vec::new();
        for o in &call.operands {
            let s = self.simplify(o, RexUnknownAs::Unknown);
            if s.is_null_literal() {
                continue;
            }
            if operands.contains(&s) {
                continue;
            }
            let not_null = !s.get_type().is_nullable();
            operands.push(s);
            if not_null {
                break;
            }
        }
        match operands.len() {
            0 => RexBuilder::make_null_literal(&call.ty),
            1 => {
                let only = operands.remove(0);
                if only.get_type() == &call.ty {
                    only
                } else {
                    RexBuilder::make_cast(call.ty.clone(), only, false)
                }
            }
            _ => {
                if operands == call.operands {
                    RexNode::Call(call.clone())
                } else {
                    RexBuilder::make_call_typed(call.ty.clone(), call.op, operands)
                }
            }
        }
    }

    fn simplify_case(self, call: &RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let n = call.operands.len();
        let mut branches: Vec<(RexNode, RexNode)> = Vec::new();
        let mut else_value: Option<RexNode> = None;
        let mut i = 0;
        while i + 1 < n {
            let cond = self.simplify(&call.operands[i], RexUnknownAs::False);
            let value = self.simplify(&call.operands[i + 1], unknown_as);
            i += 2;
            if cond.is_always_false() || cond.is_null_literal() {
                continue;
            }
            if cond.is_always_true() {
                else_value = Some(value);
                break;
            }
            branches.push((cond, value));
        }
        let else_value = match else_value {
            Some(v) => v,
            None => self.simplify(&call.operands[n - 1], unknown_as),
        };
        let mut merged: Vec<(RexNode, RexNode)> = Vec::new();
        for (c, v) in branches {
            if let Some(last) = merged.last_mut() {
                if last.1 == v {
                    let or = self
                        .builder
                        .make_call(&operators::OR, vec![last.0.clone(), c]);
                    last.0 = self.simplify(&or, RexUnknownAs::False);
                    continue;
                }
            }
            merged.push((c, v));
        }
        while let Some(last) = merged.last() {
            if last.1 == else_value {
                merged.pop();
            } else {
                break;
            }
        }
        if merged.is_empty() {
            if else_value.get_type() == &call.ty {
                return else_value;
            }
            return RexBuilder::make_cast(call.ty.clone(), else_value, false);
        }
        let mut operands = Vec::new();
        for (c, v) in merged {
            operands.push(c);
            operands.push(v);
        }
        operands.push(else_value);
        if operands == call.operands {
            return RexNode::Call(call.clone());
        }
        RexBuilder::make_call_typed(call.ty.clone(), &operators::CASE, operands)
    }
}

fn fold_literal_cast(lit: &RexLiteral, target: &RelDataType) -> Option<RexNode> {
    let tn = target.sql_type_name();
    let value = match (&lit.value, tn) {
        (LiteralValue::Null, _) => LiteralValue::Null,
        (LiteralValue::Char(s), SqlTypeName::Varchar) => LiteralValue::Char(s.clone()),
        (LiteralValue::Char(s), SqlTypeName::Char) => {
            let p = usize::try_from(target.precision()).ok();
            let len = s.chars().count();
            if p.is_some_and(|p| len > p) {
                return None;
            }
            let mut padded = s.clone();
            if let Some(p) = p {
                padded.extend(std::iter::repeat_n(' ', p.saturating_sub(len)));
            }
            LiteralValue::Char(padded)
        }
        (
            LiteralValue::Exact(d),
            SqlTypeName::Tinyint
            | SqlTypeName::Smallint
            | SqlTypeName::Integer
            | SqlTypeName::Bigint,
        ) if d.is_integer() => LiteralValue::Exact(d.clone()),
        (LiteralValue::Exact(d), SqlTypeName::Double | SqlTypeName::Float | SqlTypeName::Real) => {
            LiteralValue::Approx(d.to_string().parse().ok()?)
        }
        (LiteralValue::Approx(f), SqlTypeName::Double | SqlTypeName::Float | SqlTypeName::Real) => {
            LiteralValue::Approx(*f)
        }
        (LiteralValue::Date(d), SqlTypeName::Date) => LiteralValue::Date(d.clone()),
        (LiteralValue::Timestamp(t), SqlTypeName::Timestamp) => LiteralValue::Timestamp(t.clone()),
        (LiteralValue::Boolean(b), SqlTypeName::Boolean) => LiteralValue::Boolean(*b),
        _ => return None,
    };
    if matches!(value, LiteralValue::Null) {
        return Some(RexNode::Literal(RexLiteral {
            value,
            ty: target.with_nullability(true),
        }));
    }
    Some(RexNode::Literal(RexLiteral {
        value,
        ty: target.clone(),
    }))
}

pub fn decompose_conjunction(e: &RexNode, terms: &mut Vec<RexNode>, not_terms: &mut Vec<RexNode>) {
    if e.is_always_true() {
        return;
    }
    match e {
        RexNode::Call(c) if c.op.kind == SqlKind::And => {
            for o in &c.operands {
                decompose_conjunction(o, terms, not_terms);
            }
        }
        RexNode::Call(c) if c.op.kind == SqlKind::Not => {
            let inner = &c.operands[0];
            if inner.is_always_false() {
                return;
            }
            if inner.kind() == SqlKind::Or {
                for o in disjunctions(inner) {
                    if o.kind() == SqlKind::Not {
                        terms.push(o.operands()[0].clone());
                    } else {
                        not_terms.push(o);
                    }
                }
            } else {
                not_terms.push(inner.clone());
            }
        }
        other => terms.push(other.clone()),
    }
}

pub fn remove_nullability_cast(e: &RexNode) -> RexNode {
    match e {
        RexNode::Call(c) if c.op.kind == SqlKind::Cast => {
            let inner = &c.operands[0];
            if inner.get_type().equals_sans_nullability(&c.ty) {
                return inner.clone();
            }
            e.clone()
        }
        _ => e.clone(),
    }
}
