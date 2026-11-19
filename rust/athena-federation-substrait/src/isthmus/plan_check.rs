use std::collections::HashMap;
use std::ptr;

use super::extensions::{Extensions, FunctionKind};
use super::substrait_type::{SubstraitType, TypeKind};
use crate::error::SubstraitError;
use crate::proto::expression::field_reference::{ReferenceType, RootType};
use crate::proto::expression::literal::LiteralType;
use crate::proto::expression::reference_segment;
use crate::proto::expression::subquery::{set_predicate::PredicateOp, SubqueryType};
use crate::proto::expression::{
    FieldReference, IfThen, Literal, ReferenceSegment, RexType, ScalarFunction, Subquery,
    WindowFunction,
};
use crate::proto::function_argument::ArgType;
use crate::proto::join_rel::JoinType;
use crate::proto::read_rel::ReadType;
use crate::proto::rel::RelType;
use crate::proto::rel_common::EmitKind;
use crate::proto::sort_field::SortKind;
use crate::proto::{
    AggregateFunction, Expression, FunctionArgument, Plan, Rel, RelCommon, SortField, Type,
};

pub(crate) struct FieldInfo {
    pub(crate) ty: SubstraitType,
    pub(crate) nested_text: Option<String>,
}

pub(crate) struct CheckedPlan {
    pub(crate) extensions: Extensions,
    fields: HashMap<*const FieldReference, FieldInfo>,
}

impl CheckedPlan {
    pub(crate) fn field(&self, reference: &FieldReference) -> Option<&FieldInfo> {
        self.fields.get(&ptr::from_ref(reference))
    }
}

enum Halt {
    Error(SubstraitError),
    Unmodeled,
}

impl From<SubstraitError> for Halt {
    fn from(error: SubstraitError) -> Halt {
        Halt::Error(error)
    }
}

type Walk<T> = std::result::Result<T, Halt>;

type Record = Vec<Option<SubstraitType>>;

fn rex_not_set() -> Halt {
    Halt::Error(SubstraitError::IllegalArgument(
        "Unknown type: REXTYPE_NOT_SET".into(),
    ))
}

fn literal_not_set() -> Halt {
    Halt::Error(SubstraitError::IllegalState(
        "Unexpected value: LITERALTYPE_NOT_SET".into(),
    ))
}

fn unhandled_reference() -> Halt {
    Halt::Error(SubstraitError::IllegalArgument(
        "Unhandled type: REFERENCETYPE_NOT_SET".into(),
    ))
}

fn index_out_of_bounds(index: i32, length: usize) -> Halt {
    Halt::Error(SubstraitError::IndexOutOfBounds(format!(
        "Index {index} out of bounds for length {length}"
    )))
}

fn remap(record: Record, common: Option<&RelCommon>) -> Walk<Record> {
    let Some(EmitKind::Emit(emit)) = common.and_then(|c| c.emit_kind.as_ref()) else {
        return Ok(record);
    };
    emit.output_mapping
        .iter()
        .map(|i| {
            usize::try_from(*i)
                .ok()
                .and_then(|i| record.get(i).cloned())
                .ok_or(Halt::Unmodeled)
        })
        .collect()
}

fn nullable(record: Record) -> Record {
    record
        .into_iter()
        .map(|t| t.map(|t| t.as_nullable()))
        .collect()
}

enum Segment {
    Struct(i32),
    List(i32),
    Map {
        text: Option<String>,
        key: Option<SubstraitType>,
    },
}

impl Segment {
    fn text(&self) -> Option<String> {
        match self {
            Segment::Struct(offset) => Some(format!("StructField{{offset={offset}}}")),
            Segment::List(offset) => Some(format!("ListElement{{offset={offset}}}")),
            Segment::Map { text, .. } => text.as_ref().map(|k| format!("MapKey{{key={k}}}")),
        }
    }
}

impl Segment {
    fn dereference(&self, current: SubstraitType) -> Walk<SubstraitType> {
        let unsupported = |message: &str| Err(SubstraitError::Unsupported(message.into()).into());
        match (self, current.kind) {
            (Segment::Struct(o), TypeKind::Struct(fields)) => {
                if usize::try_from(*o).is_ok_and(|o| o >= fields.len()) {
                    return Err(
                        SubstraitError::IllegalArgument("Undefined struct type.".into()).into(),
                    );
                }
                usize::try_from(*o)
                    .ok()
                    .and_then(|o| fields.get(o).cloned())
                    .ok_or_else(|| index_out_of_bounds(*o, fields.len()))
            }
            (Segment::Struct(_), _) => unsupported(
                "This visitor only supports retrieving struct types. Was applied to a non-struct type.",
            ),
            (Segment::List(_), TypeKind::List(element)) => Ok(*element),
            (Segment::List(_), _) => unsupported(
                "This visitor only supports retrieving array index offsets. Was applied to a non-array type.",
            ),
            (Segment::Map { key: Some(k), .. }, TypeKind::Map { key, value }) if *k == *key => {
                Ok(*value)
            }
            (Segment::Map { .. }, TypeKind::Map { .. }) => Err(Halt::Unmodeled),
            (Segment::Map { .. }, _) => unsupported(
                "This visitor only supports retrieving map values using map keys. Was applied to a non-map type.",
            ),
        }
    }
}

fn nested_text(segments: &[Segment], ty: &SubstraitType) -> Option<String> {
    if segments.len() < 2 {
        return None;
    }
    let texts = segments
        .iter()
        .rev()
        .map(Segment::text)
        .collect::<Option<Vec<_>>>()?;
    Some(format!(
        "FieldReference{{segments=[{}], type={ty}}}",
        texts.join(", ")
    ))
}

fn literal_text(literal: &Literal) -> Option<String> {
    let nullable = literal.nullable;
    match literal.literal_type.as_ref()? {
        LiteralType::I8(v) => Some(format!("I8Literal{{nullable={nullable}, value={v}}}")),
        LiteralType::I16(v) => Some(format!("I16Literal{{nullable={nullable}, value={v}}}")),
        LiteralType::I32(v) => Some(format!("I32Literal{{nullable={nullable}, value={v}}}")),
        LiteralType::I64(v) => Some(format!("I64Literal{{nullable={nullable}, value={v}}}")),
        LiteralType::String(v) => Some(format!("StrLiteral{{nullable={nullable}, value={v}}}")),
        _ => None,
    }
}

struct Checker {
    extensions: Extensions,
    fields: HashMap<*const FieldReference, FieldInfo>,
}

pub(crate) fn check(plan: &Plan) -> crate::error::Result<CheckedPlan> {
    let mut checker = Checker {
        extensions: Extensions::from_plan(plan)?,
        fields: HashMap::new(),
    };
    match checker.plan(plan) {
        Ok(()) | Err(Halt::Unmodeled) => Ok(CheckedPlan {
            extensions: checker.extensions,
            fields: checker.fields,
        }),
        Err(Halt::Error(error)) => Err(error),
    }
}

impl Checker {
    fn plan(&mut self, plan: &Plan) -> Walk<()> {
        for plan_rel in &plan.relations {
            match &plan_rel.rel_type {
                Some(crate::proto::plan_rel::RelType::Root(root)) => {
                    self.rel(root.input.as_ref().ok_or(Halt::Unmodeled)?)?;
                }
                _ => return Err(Halt::Unmodeled),
            }
        }
        Ok(())
    }

    fn ty(&self, ty: Option<&Type>) -> Walk<SubstraitType> {
        match ty {
            Some(t) => Ok(SubstraitType::from_proto(t, &self.extensions)?),
            None => Err(SubstraitError::Unsupported("Type is not set: ".into()).into()),
        }
    }

    fn input(&mut self, rel: Option<&Rel>) -> Walk<Record> {
        let record = self.rel(rel.ok_or(Halt::Unmodeled)?)?;
        record.ok_or(Halt::Unmodeled)
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    fn rel(&mut self, rel: &Rel) -> Walk<Option<Record>> {
        let record = match rel.rel_type.as_ref().ok_or(Halt::Unmodeled)? {
            RelType::Read(r) => {
                if r.filter.is_some() || r.best_effort_filter.is_some() || r.projection.is_some() {
                    return Err(Halt::Unmodeled);
                }
                let schema = r.base_schema.as_ref().ok_or(Halt::Unmodeled)?;
                let st = schema.r#struct.as_ref().ok_or(Halt::Unmodeled)?;
                let mut record = Vec::with_capacity(st.types.len());
                for t in &st.types {
                    record.push(Some(self.ty(Some(t))?));
                }
                match &r.read_type {
                    Some(ReadType::NamedTable(_)) | None => {}
                    Some(ReadType::VirtualTable(v)) => {
                        for row in &v.values {
                            for l in &row.fields {
                                self.literal(l)?;
                            }
                        }
                    }
                    Some(_) => return Err(Halt::Unmodeled),
                }
                remap(record, r.common.as_ref())?
            }
            RelType::Filter(f) => {
                let record = self.input(f.input.as_deref())?;
                self.expression(f.condition.as_deref(), &record)?;
                remap(record, f.common.as_ref())?
            }
            RelType::Fetch(f) => {
                let record = self.input(f.input.as_deref())?;
                remap(record, f.common.as_ref())?
            }
            RelType::Sort(s) => {
                let record = self.input(s.input.as_deref())?;
                for field in &s.sorts {
                    self.sort_field(field, &record)?;
                }
                remap(record, s.common.as_ref())?
            }
            RelType::Project(p) => {
                let mut record = self.input(p.input.as_deref())?;
                let mut types = Vec::with_capacity(p.expressions.len());
                for e in &p.expressions {
                    types.push(self.expression(Some(e), &record)?);
                }
                record.extend(types);
                remap(record, p.common.as_ref())?
            }
            RelType::Join(j) => {
                let left = self.input(j.left.as_deref())?;
                let right = self.input(j.right.as_deref())?;
                let both: Record = left.iter().chain(&right).cloned().collect();
                self.expression(j.expression.as_deref(), &both)?;
                let record = match JoinType::try_from(j.r#type) {
                    Ok(JoinType::Inner) => both.clone(),
                    Ok(JoinType::Outer) => nullable(both.clone()),
                    Ok(JoinType::Left) => left.into_iter().chain(nullable(right)).collect(),
                    Ok(JoinType::Right) => nullable(left).into_iter().chain(right).collect(),
                    Ok(JoinType::LeftSemi | JoinType::LeftAnti) => left,
                    _ => return Err(Halt::Unmodeled),
                };
                if let Some(post) = j.post_join_filter.as_deref() {
                    self.expression(Some(post), &both)?;
                }
                remap(record, j.common.as_ref())?
            }
            RelType::Cross(c) => {
                let mut record = self.input(c.left.as_deref())?;
                record.extend(self.input(c.right.as_deref())?);
                remap(record, c.common.as_ref())?
            }
            RelType::Aggregate(a) => return self.aggregate(a),
            RelType::Set(s) => {
                for input in &s.inputs {
                    self.rel(input)?;
                }
                return Ok(None);
            }
            _ => return Err(Halt::Unmodeled),
        };
        Ok(Some(record))
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    fn aggregate(&mut self, a: &crate::proto::AggregateRel) -> Walk<Option<Record>> {
        if !a.grouping_expressions.is_empty()
            || a.groupings
                .iter()
                .any(|g| !g.expression_references.is_empty())
        {
            return Err(Halt::Unmodeled);
        }
        let input = self.input(a.input.as_deref())?;
        let mut keys: Vec<&Expression> = Vec::new();
        let mut record = Vec::new();
        for g in &a.groupings {
            for e in &g.grouping_expressions {
                let ty = self.expression(Some(e), &input)?;
                if !keys.contains(&e) {
                    keys.push(e);
                    record.push(ty);
                }
            }
        }
        for m in &a.measures {
            let function = m.measure.as_ref().ok_or(Halt::Unmodeled)?;
            record.push(Some(self.aggregate_function(function, &input)?));
            if let Some(filter) = &m.filter {
                self.expression(Some(filter), &input)?;
            }
        }
        if a.groupings.len() > 1 {
            return Ok(None);
        }
        Ok(Some(remap(record, a.common.as_ref())?))
    }

    fn aggregate_function(&mut self, f: &AggregateFunction, root: &Record) -> Walk<SubstraitType> {
        self.extensions
            .check_function(FunctionKind::Aggregate, f.function_reference)?;
        self.arguments(&f.arguments, root)?;
        let output = self.ty(f.output_type.as_ref())?;
        for s in &f.sorts {
            self.sort_field(s, root)?;
        }
        Ok(output)
    }

    fn sort_field(&mut self, field: &SortField, root: &Record) -> Walk<()> {
        self.expression(field.expr.as_ref(), root)?;
        match field.sort_kind {
            Some(SortKind::Direction(1..=4)) => Ok(()),
            _ => Err(Halt::Unmodeled),
        }
    }

    fn arguments(&mut self, args: &[FunctionArgument], root: &Record) -> Walk<()> {
        for a in args {
            match &a.arg_type {
                Some(ArgType::Value(e)) => {
                    self.expression(Some(e), root)?;
                }
                Some(ArgType::Type(t)) => {
                    self.ty(Some(t))?;
                }
                Some(ArgType::Enum(_)) => return Err(Halt::Unmodeled),
                None => {
                    return Err(SubstraitError::Unsupported(
                        "Unable to convert FunctionArgument .".into(),
                    )
                    .into())
                }
            }
        }
        Ok(())
    }

    fn scalar_function(&mut self, f: &ScalarFunction, root: &Record) -> Walk<SubstraitType> {
        self.extensions
            .check_function(FunctionKind::Scalar, f.function_reference)?;
        self.arguments(&f.arguments, root)?;
        self.ty(f.output_type.as_ref())
    }

    fn window_function(&mut self, w: &WindowFunction, root: &Record) -> Walk<SubstraitType> {
        self.extensions
            .check_function(FunctionKind::Window, w.function_reference)?;
        self.arguments(&w.arguments, root)?;
        for p in &w.partitions {
            self.expression(Some(p), root)?;
        }
        for s in &w.sorts {
            self.sort_field(s, root)?;
        }
        let output = self.ty(w.output_type.as_ref())?;
        if !matches!(w.bounds_type, 1 | 2) {
            return Err(Halt::Unmodeled);
        }
        Ok(output)
    }

    fn literal(&mut self, l: &Literal) -> Walk<Option<SubstraitType>> {
        #[allow(
            deprecated,
            reason = "reads deprecated Substrait proto fields that the Java converter still reads"
        )]
        match l.literal_type.as_ref().ok_or_else(literal_not_set)? {
            LiteralType::List(list) => {
                for v in &list.values {
                    self.literal(v)?;
                }
            }
            LiteralType::Struct(s) => {
                for v in &s.fields {
                    self.literal(v)?;
                }
            }
            LiteralType::Map(m) => {
                for kv in &m.key_values {
                    self.literal(kv.key.as_ref().ok_or(Halt::Unmodeled)?)?;
                    self.literal(kv.value.as_ref().ok_or(Halt::Unmodeled)?)?;
                }
            }
            LiteralType::Decimal(d) if d.value.len() != 16 => return Err(Halt::Unmodeled),
            LiteralType::IntervalCompound(_) => return Err(Halt::Unmodeled),
            _ => {}
        }
        Ok(SubstraitType::of_literal(l, &self.extensions)?)
    }

    fn expression(&mut self, e: Option<&Expression>, root: &Record) -> Walk<Option<SubstraitType>> {
        let rex = e
            .and_then(|e| e.rex_type.as_ref())
            .ok_or_else(rex_not_set)?;
        let ty = match rex {
            RexType::Literal(l) => self.literal(l)?,
            RexType::Selection(f) => self.field_reference(f, root)?,
            RexType::ScalarFunction(f) => Some(self.scalar_function(f, root)?),
            RexType::WindowFunction(w) => Some(self.window_function(w, root)?),
            RexType::IfThen(it) => self.if_then(it, root)?,
            RexType::SwitchExpression(sw) => {
                for clause in &sw.ifs {
                    self.literal(clause.r#if.as_ref().ok_or_else(literal_not_set)?)?;
                    self.expression(clause.then.as_ref(), root)?;
                }
                self.expression(sw.r#match.as_deref(), root)?;
                self.expression(sw.r#else.as_deref(), root)?
            }
            RexType::SingularOrList(or) => {
                for o in &or.options {
                    self.expression(Some(o), root)?;
                }
                self.expression(or.value.as_deref(), root)?;
                Some(SubstraitType::boolean(true))
            }
            RexType::Cast(c) => {
                let ty = self.ty(c.r#type.as_ref())?;
                self.expression(c.input.as_deref(), root)?;
                if !matches!(c.failure_behavior, 0..=2) {
                    return Err(Halt::Unmodeled);
                }
                Some(ty)
            }
            RexType::Subquery(q) => self.subquery(q, root)?,
            RexType::MultiOrList(_) => return Err(Halt::Unmodeled),
            RexType::Nested(_) => {
                return Err(SubstraitError::IllegalArgument("Unknown type: NESTED".into()).into())
            }
            RexType::DynamicParameter(_) => {
                return Err(SubstraitError::IllegalArgument(
                    "Unknown type: DYNAMIC_PARAMETER".into(),
                )
                .into())
            }
            #[allow(
                deprecated,
                reason = "reads deprecated Substrait proto fields that the Java converter still reads"
            )]
            RexType::Enum(_) => {
                return Err(SubstraitError::Unsupported("Unsupported type: ENUM".into()).into())
            }
        };
        Ok(ty)
    }

    fn if_then(&mut self, it: &IfThen, root: &Record) -> Walk<Option<SubstraitType>> {
        let mut thens = Vec::with_capacity(it.ifs.len());
        for clause in &it.ifs {
            self.expression(clause.r#if.as_ref(), root)?;
            thens.push(self.expression(clause.then.as_ref(), root)?);
        }
        let otherwise = self.expression(it.r#else.as_deref(), root)?;
        let then_nullable = thens
            .iter()
            .map(|t| t.as_ref().map(|t| t.nullable))
            .collect::<Option<Vec<bool>>>();
        Ok(match (otherwise, then_nullable) {
            (Some(t), Some(n)) if n.contains(&true) => Some(t.as_nullable()),
            (Some(t), Some(_)) => Some(t),
            _ => None,
        })
    }

    fn subquery(&mut self, q: &Subquery, root: &Record) -> Walk<Option<SubstraitType>> {
        match q.subquery_type.as_ref().ok_or(Halt::Unmodeled)? {
            SubqueryType::SetPredicate(p) => {
                self.rel(p.tuples.as_deref().ok_or(Halt::Unmodeled)?)?;
                match PredicateOp::try_from(p.predicate_op) {
                    Ok(PredicateOp::Exists | PredicateOp::Unique) => {
                        Ok(Some(SubstraitType::boolean(false)))
                    }
                    Ok(PredicateOp::Unspecified) => Err(SubstraitError::Unsupported(
                        "Cannot handle SetPredicate when PredicateOp is PREDICATE_OP_UNSPECIFIED."
                            .into(),
                    )
                    .into()),
                    Err(_) => Err(Halt::Unmodeled),
                }
            }
            SubqueryType::Scalar(s) => {
                let record = self.input(s.input.as_deref())?;
                if record.len() != 1 {
                    return Err(SubstraitError::Unsupported(
                        "Scalar subquery must have exactly one field".into(),
                    )
                    .into());
                }
                Ok(None)
            }
            SubqueryType::InPredicate(p) => {
                self.rel(p.haystack.as_deref().ok_or(Halt::Unmodeled)?)?;
                for n in &p.needles {
                    self.expression(Some(n), root)?;
                }
                Ok(Some(SubstraitType::boolean(false)))
            }
            SubqueryType::SetComparison(_) => Err(Halt::Unmodeled),
        }
    }

    fn segments(&mut self, first: &ReferenceSegment) -> Walk<Vec<Segment>> {
        let mut segments = Vec::new();
        let mut next = Some(first);
        while let Some(segment) = next {
            let (parsed, child) = match segment
                .reference_type
                .as_ref()
                .ok_or_else(unhandled_reference)?
            {
                reference_segment::ReferenceType::MapKey(m) => {
                    let key = m.map_key.as_ref().ok_or(Halt::Unmodeled)?;
                    let key_type = self.literal(key)?;
                    (
                        Segment::Map {
                            text: literal_text(key),
                            key: key_type,
                        },
                        m.child.as_deref(),
                    )
                }
                reference_segment::ReferenceType::StructField(s) => {
                    (Segment::Struct(s.field), s.child.as_deref())
                }
                reference_segment::ReferenceType::ListElement(l) => {
                    (Segment::List(l.offset), l.child.as_deref())
                }
            };
            segments.push(parsed);
            next = child;
        }
        Ok(segments)
    }

    fn field_reference(
        &mut self,
        f: &FieldReference,
        root: &Record,
    ) -> Walk<Option<SubstraitType>> {
        let segment = match &f.reference_type {
            Some(ReferenceType::DirectReference(s)) => s,
            Some(ReferenceType::MaskedReference(_)) => {
                return Err(SubstraitError::IllegalArgument(
                    "Unsupported type: MASKED_REFERENCE".into(),
                )
                .into())
            }
            None => return Err(unhandled_reference()),
        };
        match &f.root_type {
            Some(RootType::RootReference(_)) => {}
            Some(RootType::OuterReference(_)) => return Ok(None),
            Some(RootType::Expression(_)) => return Err(Halt::Unmodeled),
            None => {
                return Err(SubstraitError::IllegalArgument(
                    "Unhandled type: ROOTTYPE_NOT_SET".into(),
                )
                .into())
            }
        }
        let segments = self.segments(segment)?;
        let mut path = segments.iter();
        let Some(Segment::Struct(offset)) = path.next() else {
            return Err(Halt::Unmodeled);
        };
        if usize::try_from(*offset).is_ok_and(|o| o >= root.len()) {
            return Err(SubstraitError::IllegalArgument(format!(
                "Field reference offset ({offset}) must be less than number of fields in struct ({})",
                root.len()
            ))
            .into());
        }
        let mut ty = usize::try_from(*offset)
            .ok()
            .and_then(|o| root.get(o))
            .ok_or_else(|| index_out_of_bounds(*offset, root.len()))?
            .clone();
        for segment in path {
            ty = Some(segment.dereference(ty.ok_or(Halt::Unmodeled)?)?);
        }
        if let Some(ty) = &ty {
            let nested_text = nested_text(&segments, ty);
            self.fields.insert(
                ptr::from_ref(f),
                FieldInfo {
                    ty: ty.clone(),
                    nested_text,
                },
            );
        }
        Ok(ty)
    }
}
