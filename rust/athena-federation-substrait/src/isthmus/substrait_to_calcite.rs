use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use super::expression_converter::{sort_direction, ExpressionConverter};
use super::function_mappings::FunctionDecl;
use super::type_converter::{is_nullable, named_struct_to_calcite, to_calcite};
use crate::calcite::rel::{AggregateCall, JoinType, RelFieldCollation, RelNode, SetOpKind};
use crate::calcite::rel_builder::{RelBuilder, SortKey};
use crate::calcite::rel_data_type::{RelDataType, RelDataTypeBuilder, RelDataTypeSystem};
use crate::calcite::rex::{Direction, NullDirection, RexBuilder, RexLiteral, RexNode};
use crate::calcite::sql_dialect::SqlDialect;
use crate::error::{Result, SubstraitError};
use crate::java_cast::long_to_int;
use crate::proto::aggregate_function::AggregationInvocation;
use crate::proto::aggregate_rel::Measure;
use crate::proto::extensions::simple_extension_declaration::MappingType;
use crate::proto::fetch_rel::{CountMode, OffsetMode};
use crate::proto::join_rel::JoinType as ProtoJoinType;
use crate::proto::read_rel::ReadType;
use crate::proto::rel::RelType;
use crate::proto::rel_common::EmitKind;
use crate::proto::set_rel::SetOp;
use crate::proto::sort_field::SortDirection;
use crate::proto::{
    AggregateRel, Expression, FetchRel, FilterRel, JoinRel, Plan, ProjectRel, ReadRel, Rel,
    RelCommon, SetRel, SortRel,
};

pub(crate) struct OuterFrame {
    pub(crate) row_type: RelDataType,
    pub(crate) correl: Option<usize>,
}

pub(crate) struct SubstraitToCalcite {
    system: RelDataTypeSystem,
    rex_builder: RexBuilder,
    rel_builder: RelBuilder,
    functions: HashMap<u32, FunctionDecl>,
    outer: RefCell<Vec<OuterFrame>>,
    used_correl: RefCell<Vec<usize>>,
    next_correl: Cell<usize>,
}

fn expression_row_type(node: &RelNode) -> RelDataType {
    match node {
        RelNode::Aggregate {
            input,
            group_set,
            group_sets,
            row_type,
            ..
        } if group_sets.len() > 1 => {
            let input_fields = input.row_type().field_list();
            let mut b = RelDataTypeBuilder::new();
            for (i, f) in row_type.field_list().iter().enumerate() {
                let ty = match group_set.get(i).and_then(|g| input_fields.get(*g)) {
                    Some(source) => source.get_type().clone(),
                    None => f.get_type().clone(),
                };
                b.add(f.name(), ty);
            }
            b.build()
        }
        RelNode::Filter { input, .. } | RelNode::Sort { input, .. } => expression_row_type(input),
        other => other.row_type().clone(),
    }
}

fn rel_type_case_name(rel: &Rel) -> &'static str {
    match &rel.rel_type {
        None => "RELTYPE_NOT_SET",
        Some(RelType::Read(_)) => "READ",
        Some(RelType::Filter(_)) => "FILTER",
        Some(RelType::Fetch(_)) => "FETCH",
        Some(RelType::Aggregate(_)) => "AGGREGATE",
        Some(RelType::Sort(_)) => "SORT",
        Some(RelType::Join(_)) => "JOIN",
        Some(RelType::Project(_)) => "PROJECT",
        Some(RelType::Set(_)) => "SET",
        Some(RelType::ExtensionSingle(_)) => "EXTENSION_SINGLE",
        Some(RelType::ExtensionMulti(_)) => "EXTENSION_MULTI",
        Some(RelType::ExtensionLeaf(_)) => "EXTENSION_LEAF",
        Some(RelType::Cross(_)) => "CROSS",
        Some(RelType::Reference(_)) => "REFERENCE",
        Some(RelType::Write(_)) => "WRITE",
        Some(RelType::Ddl(_)) => "DDL",
        Some(RelType::Update(_)) => "UPDATE",
        Some(RelType::HashJoin(_)) => "HASH_JOIN",
        Some(RelType::MergeJoin(_)) => "MERGE_JOIN",
        Some(RelType::NestedLoopJoin(_)) => "NESTED_LOOP_JOIN",
        Some(RelType::Window(_)) => "WINDOW",
        Some(RelType::Exchange(_)) => "EXCHANGE",
        Some(RelType::Expand(_)) => "EXPAND",
    }
}

fn remap_of(common: Option<&RelCommon>) -> Option<Vec<i32>> {
    match common.and_then(|c| c.emit_kind.as_ref()) {
        Some(EmitKind::Emit(e)) => Some(e.output_mapping.clone()),
        _ => None,
    }
}

fn input_of(input: Option<&Rel>) -> &Rel {
    static DEFAULT: Rel = Rel { rel_type: None };
    input.unwrap_or(&DEFAULT)
}

impl SubstraitToCalcite {
    pub(crate) fn new(plan: &Plan, dialect: &SqlDialect) -> SubstraitToCalcite {
        let system = dialect.type_system();
        let rex_builder = RexBuilder::new(system);
        let mut functions = HashMap::new();
        for ext in &plan.extensions {
            if let Some(MappingType::ExtensionFunction(f)) = &ext.mapping_type {
                functions.insert(f.function_anchor, FunctionDecl::from_key(&f.name));
            }
        }
        SubstraitToCalcite {
            system,
            rex_builder,
            rel_builder: RelBuilder::new(RexBuilder::new(RelDataTypeSystem::Default)),
            functions,
            outer: RefCell::new(Vec::new()),
            used_correl: RefCell::new(Vec::new()),
            next_correl: Cell::new(0),
        }
    }

    pub(crate) fn convert_sub_query(
        &self,
        rel: &Rel,
        outer_row_type: &RelDataType,
    ) -> Result<RelNode> {
        self.outer.borrow_mut().push(OuterFrame {
            row_type: outer_row_type.clone(),
            correl: None,
        });
        let converted = self.convert(rel);
        let frame = self.outer.borrow_mut().pop();
        if let Some(id) = frame.and_then(|f| f.correl) {
            self.used_correl.borrow_mut().push(id);
        }
        converted
    }

    pub(crate) fn outer_field(&self, steps_out: u32, index: usize) -> Result<RexNode> {
        let mut frames = self.outer.borrow_mut();
        let len = frames.len();
        let position = usize::try_from(steps_out)
            .ok()
            .filter(|s| *s >= 1 && *s <= len)
            .map(|s| len - s)
            .ok_or_else(|| {
                SubstraitError::illegal_argument(format!(
                    "No outer row type found for steps out {steps_out}"
                ))
            })?;
        let frame = &mut frames[position];
        let fields = frame.row_type.field_list();
        let ty = fields
            .get(index)
            .map(|f| f.get_type().clone())
            .ok_or_else(|| {
                SubstraitError::index_out_of_bounds(format!(
                    "Index {index} out of bounds for length {}",
                    fields.len()
                ))
            })?;
        let correl = if let Some(c) = frame.correl {
            c
        } else {
            let c = self.next_correl.get();
            self.next_correl.set(c + 1);
            frame.correl = Some(c);
            c
        };
        Ok(RexNode::CorrelField { correl, index, ty })
    }

    fn expressions(&self) -> ExpressionConverter<'_> {
        ExpressionConverter {
            system: self.system,
            rex_builder: self.rex_builder,
            functions: &self.functions,
            rels: self,
        }
    }

    pub(crate) fn convert_plan(plan: &Plan, dialect: &SqlDialect) -> Result<RelNode> {
        let converter = SubstraitToCalcite::new(plan, dialect);
        let mut roots = Vec::with_capacity(plan.relations.len());
        for plan_rel in &plan.relations {
            let input = match &plan_rel.rel_type {
                Some(crate::proto::plan_rel::RelType::Root(r)) => r.input.as_ref(),
                _ => None,
            };
            roots.push(converter.convert(input_of(input))?);
        }
        roots.into_iter().next().ok_or_else(|| {
            SubstraitError::index_out_of_bounds("Index 0 out of bounds for length 0")
        })
    }

    fn check(rel: &Rel) -> Result<&Rel> {
        match &rel.rel_type {
            Some(
                RelType::Read(_)
                | RelType::Filter(_)
                | RelType::Fetch(_)
                | RelType::Aggregate(_)
                | RelType::Sort(_)
                | RelType::Project(_)
                | RelType::Join(_)
                | RelType::Cross(_)
                | RelType::Set(_),
            ) => Ok(rel),
            Some(
                RelType::Expand(_)
                | RelType::ExtensionSingle(_)
                | RelType::ExtensionMulti(_)
                | RelType::ExtensionLeaf(_)
                | RelType::HashJoin(_)
                | RelType::MergeJoin(_)
                | RelType::NestedLoopJoin(_)
                | RelType::Window(_)
                | RelType::Write(_)
                | RelType::Ddl(_)
                | RelType::Update(_),
            ) => Err(SubstraitError::unsupported(format!(
                "Rel of type {} is not supported by the Rust Substrait converter",
                rel_type_case_name(rel)
            ))),
            _ => Err(SubstraitError::unsupported(format!(
                "Unsupported RelTypeCase of {}",
                rel_type_case_name(rel)
            ))),
        }
    }

    pub(crate) fn convert(&self, rel: &Rel) -> Result<RelNode> {
        let rel = Self::check(rel)?;
        match &rel.rel_type {
            Some(RelType::Read(r)) => self.read(r),
            Some(RelType::Filter(f)) => self.filter(f),
            Some(RelType::Fetch(f)) => self.fetch(f),
            Some(RelType::Aggregate(a)) => self.aggregate(a),
            Some(RelType::Sort(s)) => self.sort(s),
            Some(RelType::Project(p)) => self.project(p),
            Some(RelType::Join(j)) => self.join(j),
            Some(RelType::Cross(c)) => {
                let left = self.convert(input_of(c.left.as_deref()))?;
                let right = self.convert(input_of(c.right.as_deref()))?;
                let condition = self.rex_builder.make_bool_literal(true);
                let node = self
                    .rel_builder
                    .join(left, right, JoinType::Inner, &condition);
                self.apply_remap(node, remap_of(c.common.as_ref()))
            }
            Some(RelType::Set(s)) => self.set(s),
            _ => Err(SubstraitError::unsupported(format!(
                "Unsupported RelTypeCase of {}",
                rel_type_case_name(rel)
            ))),
        }
    }

    fn apply_remap(&self, node: RelNode, remap: Option<Vec<i32>>) -> Result<RelNode> {
        let Some(indices) = remap else {
            return Ok(node);
        };
        let fields = node.row_type().field_list().to_vec();
        let mut exprs = Vec::with_capacity(indices.len());
        for i in indices {
            let index = usize::try_from(i).map_err(|_| {
                SubstraitError::index_out_of_bounds(format!(
                    "Index {i} out of bounds for length {}",
                    fields.len()
                ))
            })?;
            let f = fields.get(index).ok_or_else(|| {
                SubstraitError::index_out_of_bounds(format!(
                    "Index {index} out of bounds for length {}",
                    fields.len()
                ))
            })?;
            exprs.push(RexBuilder::make_input_ref(index, f.get_type().clone()));
        }
        Ok(self.rel_builder.project(node, &exprs))
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    fn read(&self, r: &ReadRel) -> Result<RelNode> {
        let schema = r.base_schema.clone().unwrap_or_default();
        let st = schema.r#struct.clone().unwrap_or_default();
        let row_type = named_struct_to_calcite(
            self.system,
            &schema.names,
            &st.types,
            is_nullable(st.nullability),
        )?;
        let node = match &r.read_type {
            Some(ReadType::NamedTable(t)) => RelBuilder::scan(t.names.clone(), row_type),
            Some(ReadType::VirtualTable(v)) if v.values.is_empty() => {
                RelBuilder::values(row_type, Vec::new())
            }
            Some(ReadType::VirtualTable(v)) => {
                let mut tuples = Vec::with_capacity(v.values.len());
                let ex = self.expressions();
                for row in &v.values {
                    let mut tuple: Vec<RexLiteral> = Vec::with_capacity(row.fields.len());
                    for l in &row.fields {
                        match ex.literal(l)? {
                            RexNode::Literal(lit) => tuple.push(lit),
                            _ => {
                                return Err(SubstraitError::unsupported(
                                    "VirtualTableScan only supports literal values, found: org.apache.calcite.rex.RexCall",
                                ))
                            }
                        }
                    }
                    tuples.push(tuple);
                }
                return Ok(RelBuilder::values(row_type, tuples));
            }
            Some(ReadType::LocalFiles(_)) => {
                return Err(SubstraitError::unsupported(
                    "Rel LocalFiles of type io.substrait.relation.LocalFiles not handled by visitor type io.substrait.isthmus.SubstraitRelNodeConverter.",
                ))
            }
            Some(ReadType::ExtensionTable(_) | ReadType::IcebergTable(_)) => {
                return Err(SubstraitError::unsupported(
                    "Rel ExtensionTable of type io.substrait.relation.ExtensionTable not handled by visitor type io.substrait.isthmus.SubstraitRelNodeConverter.",
                ))
            }
            None => RelBuilder::values(row_type, Vec::new()),
        };
        self.apply_remap(node, remap_of(r.common.as_ref()))
    }

    fn filter(&self, f: &FilterRel) -> Result<RelNode> {
        let input = self.convert(input_of(f.input.as_deref()))?;
        let condition = f
            .condition
            .as_deref()
            .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
        let input_type = expression_row_type(&input);
        let before = self.used_correl.borrow().len();
        let converted = self.expressions().convert(condition, &input_type);
        let variables_set: Vec<usize> = self.used_correl.borrow_mut().split_off(before);
        let rex = converted?;
        let node = self
            .rel_builder
            .filter_correlated(input, variables_set, vec![rex]);
        self.apply_remap(node, remap_of(f.common.as_ref()))
    }

    fn join(&self, j: &JoinRel) -> Result<RelNode> {
        let left = self.convert(input_of(j.left.as_deref()))?;
        let right = self.convert(input_of(j.right.as_deref()))?;
        let join_type = match ProtoJoinType::try_from(j.r#type) {
            Ok(ProtoJoinType::Inner) => JoinType::Inner,
            Ok(ProtoJoinType::Outer) => JoinType::Full,
            Ok(ProtoJoinType::Left) => JoinType::Left,
            Ok(ProtoJoinType::Right) => JoinType::Right,
            Ok(ProtoJoinType::LeftSemi) => JoinType::Semi,
            Ok(ProtoJoinType::LeftAnti) => JoinType::Anti,
            Ok(other) => {
                return Err(SubstraitError::unsupported(format!(
                    "Unsupported join type: {}",
                    other.as_str_name()
                )))
            }
            Err(_) => {
                return Err(SubstraitError::illegal_argument(
                    "Unknown type: UNRECOGNIZED",
                ))
            }
        };
        let mut input_type = RelDataTypeBuilder::new();
        input_type.add_all(expression_row_type(&left).field_list());
        input_type.add_all(expression_row_type(&right).field_list());
        let input_type = crate::calcite::rel::derive_join_row_type(
            &input_type.build(),
            &RelDataTypeBuilder::new().build(),
            JoinType::Inner,
        );
        let condition = match j.expression.as_deref() {
            Some(e) => self.expressions().convert(e, &input_type)?,
            None => self.rex_builder.make_bool_literal(true),
        };
        let mut node = self.rel_builder.join(left, right, join_type, &condition);
        if let Some(post) = j.post_join_filter.as_deref() {
            let row_type = node.row_type().clone();
            let rex = self.expressions().convert(post, &row_type)?;
            node = self.rel_builder.filter(node, vec![rex]);
        }
        self.apply_remap(node, remap_of(j.common.as_ref()))
    }

    fn set(&self, s: &SetRel) -> Result<RelNode> {
        let mut inputs = Vec::with_capacity(s.inputs.len());
        for input in &s.inputs {
            inputs.push(self.convert(input)?);
        }
        let (kind, all) = match SetOp::try_from(s.op) {
            Ok(SetOp::MinusPrimary) => (SetOpKind::Minus, false),
            Ok(SetOp::MinusPrimaryAll | SetOp::MinusMultiset) => (SetOpKind::Minus, true),
            Ok(SetOp::IntersectionPrimary | SetOp::IntersectionMultiset) => {
                (SetOpKind::Intersect, false)
            }
            Ok(SetOp::IntersectionMultisetAll) => (SetOpKind::Intersect, true),
            Ok(SetOp::UnionDistinct) => (SetOpKind::Union, false),
            Ok(SetOp::UnionAll) => (SetOpKind::Union, true),
            Ok(SetOp::Unspecified) | Err(_) => {
                return Err(SubstraitError::unsupported(
                    "Unknown set operation is not supported",
                ))
            }
        };
        let node = RelBuilder::set_op(kind, all, inputs)?;
        self.apply_remap(node, remap_of(s.common.as_ref()))
    }

    fn project(&self, p: &ProjectRel) -> Result<RelNode> {
        let child = self.convert(input_of(p.input.as_deref()))?;
        let row_type = expression_row_type(&child);
        let mut exprs: Vec<RexNode> = child
            .row_type()
            .field_list()
            .iter()
            .enumerate()
            .map(|(i, f)| RexBuilder::make_input_ref(i, f.get_type().clone()))
            .collect();
        let ex = self.expressions();
        for e in &p.expressions {
            exprs.push(ex.convert(e, &row_type)?);
        }
        let node = self
            .rel_builder
            .project_named(child, &exprs, Vec::new(), false);
        self.apply_remap(node, remap_of(p.common.as_ref()))
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    fn fetch(&self, f: &FetchRel) -> Result<RelNode> {
        let child = self.convert(input_of(f.input.as_deref()))?;
        let offset = match &f.offset_mode {
            Some(OffsetMode::Offset(o)) => *o,
            _ => 0,
        };
        let raw_count = match &f.count_mode {
            Some(CountMode::Count(c)) => *c,
            _ => 0,
        };
        let count = if raw_count == -1 { -1 } else { raw_count };
        if offset > i64::from(i32::MAX) {
            return Err(SubstraitError::illegal_argument(format!(
                "offset is overflowed as an integer: {offset}"
            )));
        }
        if count > i64::from(i32::MAX) {
            return Err(SubstraitError::illegal_argument(format!(
                "count is overflowed as an integer: {count}"
            )));
        }
        let offset = i64::from(long_to_int(offset));
        let count = i64::from(long_to_int(count));
        let node = self
            .rel_builder
            .sort_limit(child, Some(offset), Some(count), Vec::new());
        self.apply_remap(node, remap_of(f.common.as_ref()))
    }

    fn sort(&self, s: &SortRel) -> Result<RelNode> {
        let child = self.convert(input_of(s.input.as_deref()))?;
        let ex = self.expressions();
        let mut keys = Vec::with_capacity(s.sorts.len());
        for field in &s.sorts {
            let expr = field
                .expr
                .as_ref()
                .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
            let rex = ex.convert(expr, &expression_row_type(&child))?;
            let (direction, null_direction) = match sort_direction(field)? {
                SortDirection::AscNullsFirst => (Direction::Ascending, NullDirection::First),
                SortDirection::AscNullsLast => (Direction::Ascending, NullDirection::Last),
                SortDirection::DescNullsFirst => (Direction::Descending, NullDirection::First),
                SortDirection::DescNullsLast => (Direction::Descending, NullDirection::Last),
                _ => {
                    return Err(SubstraitError::unsupported(
                        "Unexpected Expression.SortDirection: Clustered!",
                    ))
                }
            };
            keys.push(SortKey {
                expr: rex,
                direction,
                null_direction: Some(null_direction),
            });
        }
        let node = self.rel_builder.sort_limit(child, None, None, keys);
        self.apply_remap(node, remap_of(s.common.as_ref()))
    }

    fn is_simple_field_reference(e: &Expression) -> bool {
        use crate::proto::expression::field_reference::{ReferenceType, RootType};
        use crate::proto::expression::reference_segment;
        use crate::proto::expression::RexType;
        match &e.rex_type {
            Some(RexType::Selection(f)) => {
                matches!(f.root_type, Some(RootType::RootReference(_)))
                    && matches!(&f.reference_type, Some(ReferenceType::DirectReference(s))
                        if matches!(&s.reference_type, Some(reference_segment::ReferenceType::StructField(sf)) if sf.child.is_none()))
            }
            _ => false,
        }
    }

    fn field_offset(e: &Expression) -> Option<i32> {
        use crate::proto::expression::field_reference::ReferenceType;
        use crate::proto::expression::reference_segment;
        use crate::proto::expression::RexType;
        match &e.rex_type {
            Some(RexType::Selection(f)) => match &f.reference_type {
                Some(ReferenceType::DirectReference(s)) => match &s.reference_type {
                    Some(reference_segment::ReferenceType::StructField(sf)) => Some(sf.field),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    fn field_ref_expression(offset: usize) -> Expression {
        use crate::proto::expression::field_reference::{ReferenceType, RootReference, RootType};
        use crate::proto::expression::reference_segment::{self, StructField};
        use crate::proto::expression::{FieldReference, ReferenceSegment, RexType};
        Expression {
            rex_type: Some(RexType::Selection(Box::new(FieldReference {
                reference_type: Some(ReferenceType::DirectReference(ReferenceSegment {
                    reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(
                        StructField {
                            field: i32::try_from(offset).unwrap_or(i32::MAX),
                            child: None,
                        },
                    ))),
                })),
                root_type: Some(RootType::RootReference(RootReference {})),
            }))),
        }
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    fn aggregate(&self, a: &AggregateRel) -> Result<RelNode> {
        let child = self.convert(input_of(a.input.as_deref()))?;
        let groupings: Vec<Vec<Expression>> = a
            .groupings
            .iter()
            .map(|g| g.grouping_expressions.clone())
            .collect();
        let mut measures = a.measures.clone();
        let (child, groupings) = if Self::aggregate_inputs_are_simple(&measures, &groupings) {
            (child, groupings)
        } else {
            self.project_aggregate_inputs(child, &mut measures, &groupings)?
        };
        let row_type = expression_row_type(&child);
        let ex = self.expressions();
        let mut group_exprs: Vec<RexNode> = Vec::new();
        let mut group_lists: Vec<Vec<RexNode>> = Vec::with_capacity(groupings.len());
        for g in &groupings {
            let mut list = Vec::with_capacity(g.len());
            for e in g {
                let rex = ex.convert(e, &row_type)?;
                if !group_exprs.contains(&rex) {
                    group_exprs.push(rex.clone());
                }
                list.push(rex);
            }
            group_lists.push(list);
        }
        let mut calls = Vec::with_capacity(measures.len());
        for m in &measures {
            calls.push(self.aggregate_call(&ex, m, &row_type)?);
        }
        let node = self
            .rel_builder
            .aggregate(child, &group_exprs, Some(&group_lists), calls)?;
        self.apply_remap(node, remap_of(a.common.as_ref()))
    }

    fn aggregate_inputs_are_simple(measures: &[Measure], groupings: &[Vec<Expression>]) -> bool {
        measures.iter().all(|m| {
            m.measure.as_ref().is_none_or(|f| {
                f.arguments.iter().all(|arg| match &arg.arg_type {
                    Some(crate::proto::function_argument::ArgType::Value(e)) => {
                        Self::is_simple_field_reference(e)
                    }
                    _ => false,
                }) && f
                    .sorts
                    .iter()
                    .all(|s| s.expr.as_ref().is_some_and(Self::is_simple_field_reference))
            }) && m
                .filter
                .as_ref()
                .is_none_or(Self::is_simple_field_reference)
        }) && groupings.iter().all(|g| {
            g.iter().all(Self::is_simple_field_reference) && {
                let offsets: Vec<i32> = g.iter().filter_map(Self::field_offset).collect();
                offsets.windows(2).all(|w| w[0] <= w[1])
            }
        })
    }

    fn project_aggregate_inputs(
        &self,
        child: RelNode,
        measures: &mut [Measure],
        groupings: &[Vec<Expression>],
    ) -> Result<(RelNode, Vec<Vec<Expression>>)> {
        let child_type = expression_row_type(&child);
        let mut new_expressions: Vec<RexNode> = Vec::new();
        let mut offset = child_type.field_count();
        let ex = self.expressions();
        let mut project_out =
            |e: &Expression, new_expressions: &mut Vec<RexNode>| -> Result<Expression> {
                new_expressions.push(ex.convert(e, &child_type)?);
                let r = Self::field_ref_expression(offset);
                offset += 1;
                Ok(r)
            };
        for m in measures.iter_mut() {
            if let Some(f) = m.measure.as_mut() {
                for arg in &mut f.arguments {
                    match &arg.arg_type {
                        Some(crate::proto::function_argument::ArgType::Value(e)) => {
                            if !Self::is_simple_field_reference(e) {
                                let r = project_out(e, &mut new_expressions)?;
                                arg.arg_type =
                                    Some(crate::proto::function_argument::ArgType::Value(r));
                            }
                        }
                        _ => {
                            return Err(SubstraitError::illegal_argument(
                                "cannot handle non-expression argument for aggregate",
                            ))
                        }
                    }
                }
                for s in &mut f.sorts {
                    if let Some(e) = s.expr.as_ref() {
                        if !Self::is_simple_field_reference(e) {
                            let r = project_out(e, &mut new_expressions)?;
                            s.expr = Some(r);
                        }
                    }
                }
            }
            if let Some(e) = m.filter.as_ref() {
                if !Self::is_simple_field_reference(e) {
                    let r = project_out(e, &mut new_expressions)?;
                    m.filter = Some(r);
                }
            }
        }
        let mut new_groupings = Vec::with_capacity(groupings.len());
        for g in groupings {
            let mut list = Vec::with_capacity(g.len());
            for e in g {
                list.push(project_out(e, &mut new_expressions)?);
            }
            new_groupings.push(list);
        }
        let mut exprs: Vec<RexNode> = child_type
            .field_list()
            .iter()
            .enumerate()
            .map(|(i, f)| RexBuilder::make_input_ref(i, f.get_type().clone()))
            .collect();
        exprs.extend(new_expressions);
        let projected = self
            .rel_builder
            .project_named(child, &exprs, Vec::new(), false);
        Ok((projected, new_groupings))
    }

    fn aggregate_call(
        &self,
        ex: &ExpressionConverter<'_>,
        m: &Measure,
        row_type: &RelDataType,
    ) -> Result<AggregateCall> {
        let f = m
            .measure
            .as_ref()
            .ok_or_else(|| SubstraitError::illegal_argument("Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan."))?;
        let (decl, op) = ex.aggregate_operator_for(f.function_reference)?;
        let args = ex.convert_arguments(decl, &f.arguments, row_type)?;
        let mut arg_index = Vec::with_capacity(args.len());
        for a in &args {
            match a.input_ref_index() {
                Some(i) => arg_index.push(i),
                None => {
                    return Err(SubstraitError::runtime(
                        "class org.apache.calcite.rex.RexCall cannot be cast to class org.apache.calcite.rex.RexInputRef",
                    ))
                }
            }
        }
        if !op.aggregate {
            return Err(SubstraitError::illegal_argument(format!(
                "Unable to convert non-aggregate operator: {} for substrait aggregate function {}",
                op.name, decl.name
            )));
        }
        let distinct = AggregationInvocation::try_from(f.invocation)
            .map(|i| i == AggregationInvocation::Distinct)
            .map_err(|_| SubstraitError::illegal_argument("Unknown type: UNRECOGNIZED"))?;
        let output = f
            .output_type
            .as_ref()
            .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
        let return_type = to_calcite(self.system, output)?;
        let filter_arg = match &m.filter {
            Some(e) => match ex.convert(e, row_type)?.input_ref_index() {
                Some(i) => Some(i),
                None => {
                    return Err(SubstraitError::runtime(
                        "class org.apache.calcite.rex.RexCall cannot be cast to class org.apache.calcite.rex.RexInputRef",
                    ))
                }
            },
            None => None,
        };
        let mut collation = Vec::with_capacity(f.sorts.len());
        for s in &f.sorts {
            let e = s
                .expr
                .as_ref()
                .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
            let index = ex
                .convert(e, row_type)?
                .input_ref_index()
                .ok_or_else(|| SubstraitError::runtime("class org.apache.calcite.rex.RexCall cannot be cast to class org.apache.calcite.rex.RexSlot"))?;
            let (direction, null_direction) = match sort_direction(s)? {
                SortDirection::AscNullsFirst => (Direction::Ascending, NullDirection::First),
                SortDirection::AscNullsLast => (Direction::Ascending, NullDirection::Last),
                SortDirection::DescNullsFirst => (Direction::Descending, NullDirection::First),
                SortDirection::DescNullsLast => (Direction::Descending, NullDirection::Last),
                _ => (Direction::Ascending, NullDirection::Unspecified),
            };
            collation.push(RelFieldCollation::new(index, direction, null_direction));
        }
        Ok(AggregateCall {
            op,
            distinct,
            approximate: false,
            args: arg_index,
            filter_arg,
            collation,
            ty: return_type,
            name: None,
        })
    }
}
