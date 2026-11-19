use std::cmp::Ordering;

use bigdecimal::BigDecimal;

use super::operators::{self, SqlKind, SqlOperator};
use super::rel_data_type::RelDataType;
use super::rex::{
    compose_conjunction, compose_disjunction, conjunction_refs, conjunctions, disjunctions,
    LiteralValue, RexBuilder, RexCall, RexLiteral, RexNode,
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

    fn ty(&self) -> RelDataType {
        match self.types.first() {
            None => self.ref_node.ty().clone(),
            Some(first) => {
                let nullable = self.types.iter().any(RelDataType::is_nullable);
                let mut best = first.clone();
                for t in &self.types {
                    if t.sql_type_name() == best.sql_type_name() {
                        if t.precision_exceeds(&best) {
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

#[derive(Debug, Clone, Copy)]
enum CollectedTerm {
    Node(usize),
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

    fn accept(&mut self, index: usize, term: &RexNode, new_terms: &mut Vec<CollectedTerm>) {
        if !self.accept_inner(term, new_terms) {
            new_terms.push(CollectedTerm::Node(index));
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

    fn fix(
        self,
        builder: RexBuilder,
        collected: Vec<CollectedTerm>,
        terms: Vec<RexNode>,
        unknown_as: RexUnknownAs,
    ) -> Vec<RexNode> {
        let negate = self.negate;
        let mut terms = terms.into_iter().enumerate();
        let mut builders = self.builders.into_iter().enumerate();
        collected
            .into_iter()
            .filter_map(|term| match term {
                CollectedTerm::Node(i) => terms.find(|(j, _)| *j == i).map(|(_, n)| n),
                CollectedTerm::Sarg(i) => builders
                    .find(|(j, _)| *j == i)
                    .map(|(_, b)| fix_sarg(builder, b, negate, unknown_as)),
            })
            .collect()
    }
}

fn fix_sarg(
    builder: RexBuilder,
    b: SargBuilder,
    negate: bool,
    unknown_as: RexUnknownAs,
) -> RexNode {
    let sarg = b.build(negate);
    let ty = b.ty();
    if sarg.complexity() <= 1 && simple_sarg(&sarg) {
        return sarg_ref(builder, &b.ref_node, &sarg, &ty, unknown_as);
    }
    builder.make_search(b.ref_node, sarg, ty)
}

fn simple_sarg(sarg: &Sarg) -> bool {
    sarg.is_points() || sarg.range_set.is_open_interval() || sarg.is_complemented_points()
}

fn make_sarg_literal(v: &SargValue, ty: &RelDataType) -> RexNode {
    RexBuilder::make_literal(LiteralValue::from_sarg_value(v), ty.with_nullability(false))
}

fn sarg_ref(
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
            .filter_map(|r| r.lower.endpoint())
            .map(|v| {
                builder.make_call(
                    &operators::NOT_EQUALS,
                    vec![ref_node.clone(), make_sarg_literal(v, ty)],
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

fn simple_sarg_node(
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

fn expand_search(builder: RexBuilder, call: RexCall) -> RexNode {
    match call.operands.get(1) {
        Some(RexNode::Literal(RexLiteral {
            value: LiteralValue::Sarg(sarg),
            ty,
        })) => sarg_ref(builder, &call.operands[0], sarg, ty, RexUnknownAs::Unknown),
        _ => RexNode::Call(call),
    }
}

fn rex_not(builder: RexBuilder, e: RexNode) -> RexNode {
    match e {
        RexNode::Call(mut c) if c.op.kind == SqlKind::Not && !c.operands.is_empty() => {
            c.operands.swap_remove(0)
        }
        other => builder.make_not(other),
    }
}

struct Comparison<'a> {
    ref_node: &'a RexNode,
    kind: SqlKind,
    literal: &'a RexLiteral,
}

fn comparison_of(e: &RexNode) -> Option<Comparison<'_>> {
    let call = e.as_call()?;
    if !call.op.kind.is_binary_comparison() {
        return None;
    }
    let left = &call.operands[0];
    let right = &call.operands[1];
    if let RexNode::Literal(literal) = right {
        return Some(Comparison {
            ref_node: left,
            kind: call.op.kind,
            literal,
        });
    }
    if let RexNode::Literal(literal) = left {
        return Some(Comparison {
            ref_node: right,
            kind: call.op.kind.reverse(),
            literal,
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

fn strip_true_equals(slot: &mut RexNode) {
    while let RexNode::Call(c) = slot {
        if c.op.kind != SqlKind::Equals {
            break;
        }
        let keep = if c.operands[0].is_always_true() {
            1
        } else if c.operands[1].is_always_true() {
            0
        } else {
            break;
        };
        let inner = c.operands.swap_remove(keep);
        *slot = inner;
    }
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
    let mut remove_upper = false;
    let mut remove_lower = false;
    match comparison {
        SqlKind::Equals => {
            if !r.contains(&v0) {
                return Some(builder.make_bool_literal(false));
            }
            let entry = &mut range_terms[pos];
            entry.range = SimpleRange::of(SqlKind::Equals, v0);
            let previous = std::mem::replace(&mut entry.terms, vec![term.clone()]);
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
        let entry = &mut range_terms[pos];
        let previous = std::mem::take(&mut entry.terms);
        let mut new_bounds = Vec::new();
        for e in previous {
            let drop = if remove_upper {
                is_upper_bound(&e)
            } else {
                is_lower_bound(&e)
            };
            if drop {
                replace_last(terms, &e, &true_literal);
            } else {
                new_bounds.push(e);
            }
        }
        new_bounds.push(term.clone());
        entry.range = r;
        entry.terms = new_bounds;
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

enum BooleanComparison {
    Operand,
    NotOperand,
    Constant(bool),
}

fn boolean_comparison(kind: SqlKind, literal: bool, nullable: bool) -> Option<BooleanComparison> {
    let outcome = match (literal, kind) {
        (true, SqlKind::GreaterThanOrEqual | SqlKind::Equals)
        | (false, SqlKind::NotEquals | SqlKind::GreaterThan) => BooleanComparison::Operand,
        (true, SqlKind::LessThan | SqlKind::NotEquals)
        | (false, SqlKind::Equals | SqlKind::LessThanOrEqual) => BooleanComparison::NotOperand,
        (true, SqlKind::GreaterThan) | (false, SqlKind::LessThan) if !nullable => {
            BooleanComparison::Constant(false)
        }
        (true, SqlKind::LessThanOrEqual) | (false, SqlKind::GreaterThanOrEqual) if !nullable => {
            BooleanComparison::Constant(true)
        }
        _ => return None,
    };
    Some(outcome)
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
        let simplified = s.simplify(composed, RexUnknownAs::False);
        if simplified.is_always_false() {
            return None;
        }
        Some(remove_nullability_cast(simplified))
    }

    pub fn simplify_unknown_as_false(self, e: RexNode) -> RexNode {
        self.simplify(e, RexUnknownAs::False)
    }

    pub fn simplify_preserving_type(self, e: RexNode) -> RexNode {
        let ty = e.ty().clone();
        let e2 = self.simplify(e, RexUnknownAs::Unknown);
        if e2.ty() == &ty {
            return e2;
        }
        RexBuilder::make_cast(ty, e2, false)
    }

    pub fn simplify(self, e: RexNode, unknown_as: RexUnknownAs) -> RexNode {
        if definitely_null(&e) {
            if e.ty().sql_type_name() == SqlTypeName::Boolean {
                if let Some(b) = unknown_as.to_boolean() {
                    return self.builder.make_bool_literal(b);
                }
            }
            if e.is_null_literal() {
                return e;
            }
            return RexBuilder::make_null_literal(e.ty());
        }
        let RexNode::Call(call) = e else {
            return e;
        };
        match call.op.kind {
            SqlKind::And => self.simplify_and(RexNode::Call(call), unknown_as),
            SqlKind::Or => self.simplify_or(RexNode::Call(call), unknown_as),
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
            SqlKind::MinusPrefix if call.operands[0].kind() == SqlKind::MinusPrefix => {
                let mut call = call;
                let inner = match call.operands.swap_remove(0) {
                    RexNode::Call(mut c) => c.operands.swap_remove(0),
                    other => other.operands()[0].clone(),
                };
                self.simplify(inner, unknown_as)
            }
            SqlKind::PlusPrefix => {
                let mut call = call;
                self.simplify(call.operands.swap_remove(0), unknown_as)
            }
            SqlKind::Plus | SqlKind::Minus | SqlKind::Times | SqlKind::Divide => {
                self.simplify_arithmetic(call)
            }
            _ => self.simplify_generic(call),
        }
    }

    fn simplify_list(self, terms: Vec<RexNode>, unknown_as: RexUnknownAs) -> Vec<RexNode> {
        terms
            .into_iter()
            .map(|t| self.simplify(t, unknown_as))
            .collect()
    }

    fn simplify_generic(self, e: RexCall) -> RexNode {
        let RexCall { op, operands, ty } = e;
        let operands = self.simplify_list(operands, RexUnknownAs::Unknown);
        RexBuilder::make_call_typed(ty, op, operands)
    }

    fn find_literal_index(operands: &[RexNode], value: i64) -> Option<usize> {
        let target = BigDecimal::from(value);
        operands.iter().position(|o| {
            matches!(o, RexNode::Literal(RexLiteral { value: LiteralValue::Exact(d), .. }) if *d == target)
        })
    }

    fn simplify_arithmetic(self, e: RexCall) -> RexNode {
        if e.ty.sql_type_name().family() != SqlTypeFamily::Numeric || e.operands.len() != 2 {
            return self.simplify_generic(e);
        }
        let kept = match e.op.kind {
            SqlKind::Plus => Self::find_literal_index(&e.operands, 0).map(|i| (i + 1) % 2),
            SqlKind::Minus => (Self::find_literal_index(&e.operands, 0) == Some(1)).then_some(0),
            SqlKind::Times => Self::find_literal_index(&e.operands, 1).map(|i| (i + 1) % 2),
            SqlKind::Divide => (Self::find_literal_index(&e.operands, 1) == Some(1)).then_some(0),
            _ => None,
        };
        let Some(i) = kept else {
            return self.simplify_generic(e);
        };
        let RexCall {
            mut operands, ty, ..
        } = e;
        let other = operands.swap_remove(i);
        if other.ty() == &ty {
            other
        } else {
            RexBuilder::make_cast(ty, other, false)
        }
    }

    fn simplify_like(self, mut e: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let matches_all = e.operands.len() == 2
            && matches!(
                &e.operands[1],
                RexNode::Literal(RexLiteral { value: LiteralValue::Char(s), .. }) if s == "%"
            );
        if matches_all {
            let x = e.operands.swap_remove(0);
            let eq = self
                .builder
                .make_call(&operators::EQUALS, vec![x.clone(), x]);
            return self.simplify(eq, unknown_as);
        }
        self.simplify_generic(e)
    }

    fn simplify_comparison(self, e: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let RexCall { op, operands, ty } = e;
        let original = operands.clone();
        let mut operands = self.simplify_list(operands, RexUnknownAs::Unknown);
        if operands[0] == operands[1] {
            let o0 = operands.swap_remove(0);
            let n = match op.kind {
                SqlKind::Equals | SqlKind::GreaterThanOrEqual | SqlKind::LessThanOrEqual => {
                    self.builder.make_call(
                        &operators::OR,
                        vec![
                            RexBuilder::make_null_literal(&ty),
                            self.builder.make_call(&operators::IS_NOT_NULL, vec![o0]),
                        ],
                    )
                }
                _ => self.builder.make_call(
                    &operators::AND,
                    vec![
                        RexBuilder::make_null_literal(&ty),
                        self.builder.make_call(&operators::IS_NULL, vec![o0]),
                    ],
                ),
            };
            return self.simplify(n, unknown_as);
        }
        if operands[0].ty().sql_type_name() == SqlTypeName::Boolean {
            let found = match (&operands[0], &operands[1]) {
                (left, RexNode::Literal(lit)) => Some((0, op.kind, left, lit)),
                (RexNode::Literal(lit), right) => Some((1, op.kind.reverse(), right, lit)),
                _ => None,
            };
            let outcome = found.and_then(|(index, kind, ref_node, lit)| match lit.value {
                LiteralValue::Boolean(b) => {
                    boolean_comparison(kind, b, ref_node.ty().is_nullable()).map(|o| (index, o))
                }
                _ => None,
            });
            if let Some((index, outcome)) = outcome {
                return match outcome {
                    BooleanComparison::Operand => operands.swap_remove(index),
                    BooleanComparison::NotOperand => self.simplify(
                        rex_not(self.builder, operands.swap_remove(index)),
                        unknown_as,
                    ),
                    BooleanComparison::Constant(b) => self.builder.make_bool_literal(b),
                };
            }
        }
        if let (RexNode::Literal(l0), RexNode::Literal(l1)) = (&operands[0], &operands[1]) {
            if l0.ty.equals_sans_nullability(&l1.ty) {
                if l0.is_null() || l1.is_null() {
                    return if unknown_as == RexUnknownAs::False {
                        self.builder.make_bool_literal(false)
                    } else {
                        RexBuilder::make_null_literal(&ty)
                    };
                }
                if let (Some(v0), Some(v1)) = (l0.value.to_sarg_value(), l1.value.to_sarg_value()) {
                    let c = v0.compare(&v1);
                    let r = match op.kind {
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
        if operands == original {
            RexBuilder::make_call_typed(ty, op, operands)
        } else {
            self.builder.make_call(op, operands)
        }
    }

    fn simplify_and(self, e: RexNode, unknown_as: RexUnknownAs) -> RexNode {
        let mut operands = conjunctions(e);
        operands = if unknown_as == RexUnknownAs::False && self.predicate_elimination {
            self.simplify_and_terms(operands)
        } else {
            self.simplify_list(operands, unknown_as)
        };
        let mut collected = Vec::new();
        let mut collector = SargCollector::new(true);
        for (i, t) in operands.iter().enumerate() {
            collector.accept(i, t, &mut collected);
        }
        if collector.need_to_fix() {
            operands = collector.fix(self.builder, collected, operands, unknown_as);
        }
        let mut terms = Vec::new();
        let mut not_terms = Vec::new();
        for o in operands {
            decompose_conjunction(o, &mut terms, &mut not_terms);
        }
        if unknown_as == RexUnknownAs::False {
            return self.simplify_and2_for_unknown_as_false(terms, not_terms);
        }
        self.simplify_and2(terms, not_terms)
    }

    fn simplify_and_terms(self, terms: Vec<RexNode>) -> Vec<RexNode> {
        let is_predicate = |t: &RexNode| {
            comparison_of(t).is_some() || matches!(t.kind(), SqlKind::IsNull | SqlKind::IsNotNull)
        };
        let terms: Vec<RexNode> = terms
            .into_iter()
            .map(|t| {
                if is_predicate(&t) {
                    self.simplify(t, RexUnknownAs::False)
                } else {
                    t
                }
            })
            .collect();
        terms
            .into_iter()
            .map(|t| {
                if is_predicate(&t) {
                    t
                } else {
                    self.simplify(t, RexUnknownAs::False)
                }
            })
            .collect()
    }

    fn simplify_and2(self, mut terms: Vec<RexNode>, not_terms: Vec<RexNode>) -> RexNode {
        if terms.iter().any(RexNode::is_always_false) {
            return self.builder.make_bool_literal(false);
        }
        if terms.is_empty() && not_terms.is_empty() {
            return self.builder.make_bool_literal(true);
        }
        let covered = |nd: &RexNode| conjunction_refs(nd).iter().all(|t| terms.contains(t));
        if not_terms
            .iter()
            .any(|nd| covered(nd) && !nd.ty().is_nullable())
        {
            return self.builder.make_bool_literal(false);
        }
        let (not_satisfiable, not_terms): (Vec<RexNode>, Vec<RexNode>) =
            not_terms.into_iter().partition(|nd| covered(nd));
        if let Some(first) = not_satisfiable.first() {
            let null_type = first.ty().clone();
            terms.retain(|t| !not_satisfiable.contains(t));
            terms.push(RexBuilder::make_null_literal(&null_type));
            for ns in not_satisfiable {
                if let RexNode::Call(c) = self.builder.make_call(&operators::IS_NULL, vec![ns]) {
                    terms.push(self.simplify_is(c, RexUnknownAs::Unknown));
                }
            }
        }
        for nd in not_terms {
            terms.push(self.simplify(rex_not(self.builder, nd), RexUnknownAs::Unknown));
        }
        compose_conjunction(self.builder, terms)
    }

    fn simplify_and2_for_unknown_as_false(
        self,
        mut terms: Vec<RexNode>,
        not_terms: Vec<RexNode>,
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
            if let Some(only) = terms.pop() {
                return self.simplify(only, RexUnknownAs::False);
            }
        }
        let mut state = AndState::default();
        let mut null_operands: Vec<RexNode> = Vec::new();
        let mut not_null_operands: Vec<RexNode> = Vec::new();
        let mut i = 0;
        while i < terms.len() {
            strip_true_equals(&mut terms[i]);
            match terms[i].kind() {
                k if k.is_binary_comparison() => {
                    let term = terms[i].clone();
                    if let Some(r) = self.absorb_comparison(k, &term, &mut terms, &mut state) {
                        return r;
                    }
                }
                SqlKind::IsNotNull => {
                    match terms.remove(i) {
                        RexNode::Call(mut c) => not_null_operands.push(c.operands.swap_remove(0)),
                        other => not_null_operands.push(other.operands()[0].clone()),
                    }
                    continue;
                }
                SqlKind::IsNull => {
                    null_operands.push(terms[i].operands()[0].clone());
                }
                _ => {}
            }
            i += 1;
        }
        for (ref1, ref2, eq_term) in &state.equality_terms {
            let constant = |r: &RexNode| {
                state
                    .equality_constants
                    .iter()
                    .find(|(c, _)| c == r)
                    .map(|(_, l)| l)
            };
            let Some(l1) = constant(ref1) else { continue };
            let Some(l2) = constant(ref2) else { continue };
            if l1.ty == l2.ty && l1 != l2 {
                return self.builder.make_bool_literal(false);
            }
            if let Some(p) = terms.iter().position(|t| t == eq_term) {
                terms.remove(p);
            }
        }
        for nd in &not_terms {
            if conjunction_refs(nd).iter().all(|t| terms.contains(t)) {
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
        let is_strong = |n: &RexNode| matches!(n, RexNode::InputRef { index, .. } if strong_operands.contains(index));
        if null_operands.iter().any(is_strong) {
            return self.builder.make_bool_literal(false);
        }
        let mut distinct: Vec<RexNode> = Vec::new();
        for operand in not_null_operands {
            if !distinct.contains(&operand) {
                distinct.push(operand);
            }
        }
        for operand in distinct {
            if !is_strong(&operand) {
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
                    .position(|(r, _)| r == cmp.ref_node)
                {
                    let previous = &mut state.equality_constants[pos].1;
                    let conflict = cmp.literal.ty == previous.ty && cmp.literal != previous;
                    *previous = cmp.literal.clone();
                    if conflict {
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
                        cmp.ref_node,
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

    fn simplify_or(self, e: RexNode, unknown_as: RexUnknownAs) -> RexNode {
        let terms0 = disjunctions(e);
        let terms = if self.predicate_elimination {
            let (mut nulls, rest): (Vec<RexNode>, Vec<RexNode>) = terms0
                .into_iter()
                .partition(|t| t.kind() == SqlKind::IsNull);
            nulls.extend(rest);
            nulls
        } else {
            terms0
        };
        let terms = self.simplify_list(terms, unknown_as);
        self.simplify_ors(terms, unknown_as)
    }

    fn simplify_ors(self, mut terms: Vec<RexNode>, unknown_as: RexUnknownAs) -> RexNode {
        let mut collected = Vec::new();
        let mut collector = SargCollector::new(false);
        for (i, t) in terms.iter().enumerate() {
            collector.accept(i, t, &mut collected);
        }
        if collector.need_to_fix() {
            terms = collector.fix(self.builder, collected, terms, unknown_as);
        }
        let true_literal = self.builder.make_bool_literal(true);
        let mut not_equals: Vec<(RexNode, RexNode)> = Vec::new();
        let mut i = 0;
        while i < terms.len() {
            match terms[i].kind() {
                SqlKind::Literal => {
                    if terms[i].is_null_literal() {
                        if unknown_as == RexUnknownAs::False {
                            terms.remove(i);
                            continue;
                        } else if unknown_as == RexUnknownAs::True {
                            return true_literal;
                        }
                    } else if literal_bool(&terms[i]) == Some(true) {
                        return terms.swap_remove(i);
                    } else {
                        terms.remove(i);
                        continue;
                    }
                }
                SqlKind::NotEquals => {
                    if let Some(cmp) = comparison_of(&terms[i]) {
                        let ref_node = cmp.ref_node.clone();
                        let v1 = cmp.literal.value.to_sarg_value();
                        if let Some(pos) = not_equals.iter().position(|(r, _)| r == &ref_node) {
                            let prev = &not_equals[pos].1;
                            if prev.kind() != SqlKind::IsNotNull {
                                let v2 = comparison_of(prev)
                                    .and_then(|c| c.literal.value.to_sarg_value());
                                if let (Some(v1), Some(v2)) = (v1, v2) {
                                    if v1.compare(&v2) != Ordering::Equal {
                                        let prev_position = terms.iter().position(|t| t == prev);
                                        let is_not_null = self
                                            .builder
                                            .make_call(&operators::IS_NOT_NULL, vec![ref_node]);
                                        let cond = self.simplify(
                                            self.builder.make_call(
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
                                        if let Some(p) = prev_position {
                                            terms[p] = cond;
                                        }
                                    }
                                }
                            }
                            terms.remove(i);
                            continue;
                        }
                        not_equals.push((ref_node, terms[i].clone()));
                    }
                }
                SqlKind::Not => {
                    let x = &terms[i].operands()[0];
                    if terms.contains(x) {
                        if !x.ty().is_nullable() {
                            return true_literal;
                        }
                        let x = x.clone();
                        let pos = terms.iter().position(|t| t == &x);
                        let null_literal = RexBuilder::make_null_literal(x.ty());
                        let is_not_null = self.builder.make_call(&operators::IS_NOT_NULL, vec![x]);
                        if let (Some(p), RexNode::Call(c)) = (pos, is_not_null) {
                            terms[p] = self.simplify_is(c, unknown_as);
                        }
                        terms[i] = null_literal;
                        continue;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        compose_disjunction(self.builder, terms)
    }

    fn simplify_not(self, call: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let RexCall {
            op,
            mut operands,
            ty,
        } = call;
        let a = match operands.remove(0) {
            RexNode::Call(ac) => match self.simplify_not_call(ac, unknown_as) {
                Ok(simplified) => return simplified,
                Err(ac) => RexNode::Call(ac),
            },
            RexNode::Literal(RexLiteral {
                value: LiteralValue::Boolean(b),
                ..
            }) => return self.builder.make_bool_literal(!b),
            other => other,
        };
        let original = a.clone();
        let a2 = self.simplify(a, unknown_as.negate());
        if a2 == original {
            operands.insert(0, a2);
            return RexNode::Call(RexCall { op, operands, ty });
        }
        rex_not(self.builder, a2)
    }

    fn simplify_not_call(self, ac: RexCall, unknown_as: RexUnknownAs) -> Result<RexNode, RexCall> {
        match ac.op.kind {
            SqlKind::Not => {
                let mut ac = ac;
                Ok(self.simplify(ac.operands.swap_remove(0), unknown_as))
            }
            SqlKind::Search => {
                let mut ac = ac;
                if let Some(RexNode::Literal(RexLiteral {
                    value: LiteralValue::Sarg(sarg),
                    ..
                })) = ac.operands.get_mut(1)
                {
                    **sarg = sarg.negate();
                    ac.operands.truncate(2);
                    return Ok(self.simplify_search(ac, unknown_as.negate()));
                }
                Err(ac)
            }
            SqlKind::And | SqlKind::Or => {
                let op = if ac.op.kind == SqlKind::And {
                    &operators::OR
                } else {
                    &operators::AND
                };
                let ops: Vec<RexNode> = ac
                    .operands
                    .into_iter()
                    .map(|o| self.simplify(rex_not(self.builder, o), unknown_as))
                    .collect();
                Ok(self.simplify(self.builder.make_call(op, ops), unknown_as))
            }
            SqlKind::Case => {
                let n = ac.operands.len();
                let ops: Vec<RexNode> = ac
                    .operands
                    .into_iter()
                    .enumerate()
                    .map(|(i, o)| {
                        if i % 2 == 1 || i + 1 == n {
                            rex_not(self.builder, o)
                        } else {
                            o
                        }
                    })
                    .collect();
                let c = RexBuilder::make_call_typed(ac.ty, &operators::CASE, ops);
                Ok(self.simplify(c, unknown_as))
            }
            SqlKind::In | SqlKind::NotIn => Err(ac),
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
                        let c = self.builder.make_call(op, ac.operands);
                        return Ok(self.simplify(c, unknown_as));
                    }
                }
                Err(ac)
            }
        }
    }

    fn simplify_is(self, call: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let RexCall {
            op,
            mut operands,
            ty,
        } = call;
        match self.simplify_is1(op.kind, operands.remove(0), unknown_as) {
            Ok(n) => n,
            Err(a) => {
                operands.insert(0, a);
                RexNode::Call(RexCall { op, operands, ty })
            }
        }
    }

    fn simplify_is1(
        self,
        kind: SqlKind,
        a: RexNode,
        unknown_as: RexUnknownAs,
    ) -> Result<RexNode, RexNode> {
        match (kind, unknown_as) {
            (SqlKind::IsTrue, RexUnknownAs::False) | (SqlKind::IsNotFalse, RexUnknownAs::True) => {
                Ok(self.simplify(a, unknown_as))
            }
            (SqlKind::IsFalse, RexUnknownAs::False) | (SqlKind::IsNotTrue, RexUnknownAs::True) => {
                Ok(self.simplify(rex_not(self.builder, a), unknown_as))
            }
            _ => self.simplify_is2(kind, a, unknown_as),
        }
    }

    fn simplify_is2(
        self,
        kind: SqlKind,
        a: RexNode,
        unknown_as: RexUnknownAs,
    ) -> Result<RexNode, RexNode> {
        match kind {
            SqlKind::IsNull => {
                if let Some(s) = self.simplify_is_null(&a) {
                    return Ok(s);
                }
            }
            SqlKind::IsNotNull => {
                if let Some(s) = self.simplify_is_not_null(&a) {
                    return Ok(s);
                }
            }
            SqlKind::IsTrue | SqlKind::IsNotFalse => {
                if !a.ty().is_nullable() {
                    return Ok(self.simplify(a, unknown_as));
                }
                let (inner_unknown_as, op) = if kind == SqlKind::IsTrue {
                    (RexUnknownAs::False, &operators::IS_TRUE)
                } else {
                    (RexUnknownAs::True, &operators::IS_NOT_FALSE)
                };
                let s = self.simplify(a.clone(), inner_unknown_as);
                if s == a {
                    return Err(a);
                }
                return Ok(self.builder.make_call(op, vec![s]));
            }
            SqlKind::IsFalse | SqlKind::IsNotTrue if !a.ty().is_nullable() => {
                return Ok(self.simplify(rex_not(self.builder, a), unknown_as));
            }
            _ => {}
        }
        if a.kind() == SqlKind::Not {
            if let Some(op) = operators::of_comparison_kind(kind.negate_null_safe()) {
                let inner = match a {
                    RexNode::Call(mut c) => c.operands.swap_remove(0),
                    other => other.operands()[0].clone(),
                };
                let c = self.builder.make_call(op, vec![inner]);
                return Ok(self.simplify(c, RexUnknownAs::Unknown));
            }
        }
        let a2 = self.simplify(a.clone(), RexUnknownAs::Unknown);
        if a2 != a {
            if let Some(op) = operators::of_comparison_kind(kind) {
                return Ok(self.builder.make_call(op, vec![a2]));
            }
        }
        Err(a)
    }

    fn simplify_is_not_null(self, a: &RexNode) -> Option<RexNode> {
        let a = self.simplify(a.clone(), RexUnknownAs::Unknown);
        if !a.ty().is_nullable() {
            return Some(self.builder.make_bool_literal(true));
        }
        match a {
            RexNode::Literal(l) => Some(self.builder.make_bool_literal(!l.is_null())),
            RexNode::Call(c) if is_strong_kind(c.op.kind) && c.op.kind != SqlKind::Search => {
                let mut operands = Vec::new();
                for o in c.operands {
                    match self.simplify_is_not_null(&o) {
                        None => {
                            operands.push(self.builder.make_call(&operators::IS_NOT_NULL, vec![o]));
                        }
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
        let a = self.simplify(a.clone(), RexUnknownAs::Unknown);
        if !a.ty().is_nullable() {
            return Some(self.builder.make_bool_literal(false));
        }
        match a {
            RexNode::Literal(l) => Some(self.builder.make_bool_literal(l.is_null())),
            RexNode::Call(c) if is_strong_kind(c.op.kind) && c.op.kind != SqlKind::Search => {
                let mut operands = Vec::new();
                for o in c.operands {
                    match self.simplify_is_null(&o) {
                        None => operands.push(self.builder.make_call(&operators::IS_NULL, vec![o])),
                        Some(s) => operands.push(s),
                    }
                }
                Some(compose_disjunction(self.builder, operands))
            }
            _ => None,
        }
    }

    fn simplify_search(self, mut call: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let (all_or_none, null_as, single_point) = match call.operands.get(1) {
            Some(RexNode::Literal(RexLiteral {
                value: LiteralValue::Sarg(sarg),
                ..
            })) => (
                sarg.is_all() || sarg.is_none(),
                sarg.null_as,
                sarg.is_points() && sarg.point_count() <= 1,
            ),
            _ => return RexNode::Call(call),
        };
        if all_or_none {
            let n = match &call.operands[1] {
                RexNode::Literal(RexLiteral {
                    value: LiteralValue::Sarg(sarg),
                    ..
                }) => simple_sarg_node(self.builder, &call.operands[0], sarg, unknown_as),
                _ => return RexNode::Call(call),
            };
            return self.simplify(n, unknown_as);
        }
        if null_as != RexUnknownAs::Unknown {
            if let Ok(s) = self.simplify_is1(SqlKind::IsNull, call.operands[0].clone(), unknown_as)
            {
                if s.is_always_false() {
                    if let Some(RexNode::Literal(RexLiteral {
                        value: LiteralValue::Sarg(sarg),
                        ..
                    })) = call.operands.get_mut(1)
                    {
                        sarg.null_as = RexUnknownAs::Unknown;
                    }
                    call.operands.truncate(2);
                    return self.simplify_search(call, unknown_as);
                }
            }
        } else if single_point {
            return expand_search(self.builder, call);
        }
        RexNode::Call(call)
    }

    fn simplify_cast(self, e: RexCall) -> RexNode {
        let RexCall {
            op,
            mut operands,
            ty,
        } = e;
        let operand = self.simplify(operands.remove(0), RexUnknownAs::Unknown);
        let same_or_narrows = ty == *operand.ty()
            || (ty.equals_sans_nullability(operand.ty()) && !operand.ty().is_nullable());
        if same_or_narrows && ty.sql_type_name() != SqlTypeName::Decimal {
            return operand;
        }
        if let RexNode::Literal(lit) = &operand {
            if let Some(folded) = fold_literal_cast(lit, &ty) {
                return folded;
            }
        }
        operands.insert(0, operand);
        RexBuilder::make_call_typed(ty, op, operands)
    }

    fn simplify_coalesce(self, call: RexCall) -> RexNode {
        let RexCall {
            op,
            operands: original,
            ty,
        } = call;
        let mut operands: Vec<RexNode> = Vec::new();
        for o in original {
            let s = self.simplify(o, RexUnknownAs::Unknown);
            if s.is_null_literal() {
                continue;
            }
            if operands.contains(&s) {
                continue;
            }
            let not_null = !s.ty().is_nullable();
            operands.push(s);
            if not_null {
                break;
            }
        }
        match operands.len() {
            0 => RexBuilder::make_null_literal(&ty),
            1 => {
                let only = operands.remove(0);
                if only.ty() == &ty {
                    only
                } else {
                    RexBuilder::make_cast(ty, only, false)
                }
            }
            _ => RexBuilder::make_call_typed(ty, op, operands),
        }
    }

    fn simplify_case(self, call: RexCall, unknown_as: RexUnknownAs) -> RexNode {
        let RexCall {
            op,
            operands: original,
            ty,
        } = call;
        let n = original.len();
        let mut remaining = original.into_iter();
        let mut branches: Vec<(RexNode, RexNode)> = Vec::new();
        let mut else_value: Option<RexNode> = None;
        let mut i = 0;
        while i + 1 < n {
            let (Some(cond), Some(value)) = (remaining.next(), remaining.next()) else {
                break;
            };
            let cond = self.simplify(cond, RexUnknownAs::False);
            let value = self.simplify(value, unknown_as);
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
            None => match remaining.next_back() {
                Some(last) => self.simplify(last, unknown_as),
                None => RexBuilder::make_null_literal(&ty),
            },
        };
        let mut merged: Vec<(RexNode, RexNode)> = Vec::new();
        for (c, v) in branches {
            if let Some(last) = merged.last_mut() {
                if last.1 == v {
                    let previous =
                        std::mem::replace(&mut last.0, self.builder.make_bool_literal(false));
                    let or = self.builder.make_call(&operators::OR, vec![previous, c]);
                    last.0 = self.simplify(or, RexUnknownAs::False);
                    continue;
                }
            }
            merged.push((c, v));
        }
        while merged.last().is_some_and(|last| last.1 == else_value) {
            merged.pop();
        }
        if merged.is_empty() {
            if else_value.ty() == &ty {
                return else_value;
            }
            return RexBuilder::make_cast(ty, else_value, false);
        }
        let mut operands = Vec::with_capacity(merged.len() * 2 + 1);
        for (c, v) in merged {
            operands.push(c);
            operands.push(v);
        }
        operands.push(else_value);
        RexBuilder::make_call_typed(ty, op, operands)
    }
}

fn truncate(s: &str, precision: Option<usize>) -> String {
    match precision {
        Some(precision) => s.chars().take(precision).collect(),
        None => s.to_string(),
    }
}

fn truncate_or_pad(s: &str, precision: Option<usize>) -> String {
    let mut text = truncate(s, precision);
    if let Some(precision) = precision {
        let len = text.chars().count();
        text.extend(std::iter::repeat_n(' ', precision.saturating_sub(len)));
    }
    text
}

fn char_precision(target: &RelDataType) -> Option<usize> {
    target.precision().and_then(|p| usize::try_from(p).ok())
}

fn fold_literal_cast(lit: &RexLiteral, target: &RelDataType) -> Option<RexNode> {
    let tn = target.sql_type_name();
    let value = match (&lit.value, tn) {
        (LiteralValue::Null, _) => LiteralValue::Null,
        (LiteralValue::Char(s), SqlTypeName::Varchar) => {
            LiteralValue::Char(truncate(s, char_precision(target)))
        }
        (LiteralValue::Char(s), SqlTypeName::Char) => {
            LiteralValue::Char(truncate_or_pad(s, char_precision(target)))
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

fn decompose_conjunction(e: RexNode, terms: &mut Vec<RexNode>, not_terms: &mut Vec<RexNode>) {
    if e.is_always_true() {
        return;
    }
    match e {
        RexNode::Call(c) if c.op.kind == SqlKind::And => {
            for o in c.operands {
                decompose_conjunction(o, terms, not_terms);
            }
        }
        RexNode::Call(mut c) if c.op.kind == SqlKind::Not => {
            let inner = c.operands.swap_remove(0);
            if inner.is_always_false() {
                return;
            }
            if inner.kind() == SqlKind::Or {
                for o in disjunctions(inner) {
                    match o {
                        RexNode::Call(mut n) if n.op.kind == SqlKind::Not => {
                            terms.push(n.operands.swap_remove(0));
                        }
                        other => not_terms.push(other),
                    }
                }
            } else {
                not_terms.push(inner);
            }
        }
        other => terms.push(other),
    }
}

fn remove_nullability_cast(e: RexNode) -> RexNode {
    match e {
        RexNode::Call(mut c)
            if c.op.kind == SqlKind::Cast && c.operands[0].ty().equals_sans_nullability(&c.ty) =>
        {
            c.operands.swap_remove(0)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::super::rel_data_type::RelDataTypeSystem;
    use super::*;

    fn cast_char_literal(text: &str, precision: i32) -> Option<String> {
        let literal = RexLiteral {
            value: LiteralValue::Char(text.to_string()),
            ty: RelDataType::create_sql_type_with_precision(
                RelDataTypeSystem::Default,
                SqlTypeName::Char,
                1,
            ),
        };
        let target = RelDataType::create_sql_type_with_precision(
            RelDataTypeSystem::Default,
            SqlTypeName::Char,
            precision,
        );
        match fold_literal_cast(&literal, &target)? {
            RexNode::Literal(RexLiteral {
                value: LiteralValue::Char(folded),
                ..
            }) => Some(folded),
            other => panic!("unexpected fold {other:?}"),
        }
    }

    #[test]
    fn char_cast_truncates_long_literals_by_code_points() {
        assert_eq!(Some("a".to_string()), cast_char_literal("ab", 1));
        assert_eq!(Some("hel".to_string()), cast_char_literal("hello", 3));
        assert_eq!(
            Some("\u{1F600}b".to_string()),
            cast_char_literal("\u{1F600}b", 2)
        );
        assert_eq!(
            Some("\u{e9}".to_string()),
            cast_char_literal("\u{e9}\u{1F600}", 1)
        );
        assert_eq!(
            Some("\u{1F600}".to_string()),
            cast_char_literal("\u{1F600}x", 1)
        );
        assert_eq!(
            Some("a\u{1F600}".to_string()),
            cast_char_literal("a\u{1F600}b", 2)
        );
    }

    #[test]
    fn char_cast_pads_astral_literals_by_code_points() {
        assert_eq!(
            Some("\u{1F600} ".to_string()),
            cast_char_literal("\u{1F600}", 2)
        );
        assert_eq!(
            Some("\u{1F600}  ".to_string()),
            cast_char_literal("\u{1F600}", 3)
        );
        assert_eq!(
            Some("a\u{1F600}  ".to_string()),
            cast_char_literal("a\u{1F600}", 4)
        );
    }
}
