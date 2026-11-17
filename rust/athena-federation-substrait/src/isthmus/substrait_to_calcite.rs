#![allow(deprecated)]

use std::collections::HashMap;

use super::expression_converter::{sort_direction, ExpressionConverter};
use super::function_mappings::FunctionDecl;
use super::type_converter::{is_nullable, named_struct_to_calcite, to_calcite};
use crate::calcite::rel::{AggregateCall, RelFieldCollation, RelNode};
use crate::calcite::rel_builder::{RelBuilder, SortKey};
use crate::calcite::rel_data_type::RelDataTypeSystem;
use crate::calcite::rex::{Direction, NullDirection, RexBuilder, RexLiteral, RexNode};
use crate::calcite::sql_dialect::SqlDialect;
use crate::error::{Result, SubstraitError};
use crate::proto::aggregate_function::AggregationInvocation;
use crate::proto::extensions::simple_extension_declaration::MappingType;
use crate::proto::fetch_rel::{CountMode, OffsetMode};
use crate::proto::read_rel::ReadType;
use crate::proto::rel::RelType;
use crate::proto::rel_common::EmitKind;
use crate::proto::sort_field::SortDirection;
use crate::proto::{
    AggregateRel, Expression, FetchRel, FilterRel, Plan, ProjectRel, ReadRel, Rel, RelCommon, SortRel,
};

pub(crate) struct SubstraitToCalcite {
    system: RelDataTypeSystem,
    rex_builder: RexBuilder,
    rel_builder: RelBuilder,
    functions: HashMap<u32, FunctionDecl>,
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

fn input_of(input: Option<&Rel>) -> Result<&Rel> {
    static DEFAULT: Rel = Rel { rel_type: None };
    Ok(input.unwrap_or(&DEFAULT))
}

impl SubstraitToCalcite {
    pub(crate) fn new(plan: &Plan, dialect: &SqlDialect) -> SubstraitToCalcite {
        let system = dialect.get_type_system();
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
        }
    }

    fn expressions(&self) -> ExpressionConverter<'_> {
        ExpressionConverter {
            system: self.system,
            rex_builder: self.rex_builder,
            functions: &self.functions,
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
            roots.push(converter.convert(input_of(input)?)?);
        }
        roots.into_iter().next().ok_or_else(|| {
            SubstraitError::index_out_of_bounds("Index 0 out of bounds for length 0")
        })
    }

    fn check<'r>(&self, rel: &'r Rel) -> Result<&'r Rel> {
        match &rel.rel_type {
            Some(RelType::Read(_))
            | Some(RelType::Filter(_))
            | Some(RelType::Fetch(_))
            | Some(RelType::Aggregate(_))
            | Some(RelType::Sort(_))
            | Some(RelType::Project(_)) => Ok(rel),
            Some(RelType::Join(_))
            | Some(RelType::Set(_))
            | Some(RelType::Cross(_))
            | Some(RelType::Expand(_))
            | Some(RelType::ExtensionSingle(_))
            | Some(RelType::ExtensionMulti(_))
            | Some(RelType::ExtensionLeaf(_))
            | Some(RelType::HashJoin(_))
            | Some(RelType::MergeJoin(_))
            | Some(RelType::NestedLoopJoin(_))
            | Some(RelType::Window(_))
            | Some(RelType::Write(_))
            | Some(RelType::Ddl(_))
            | Some(RelType::Update(_)) => Err(SubstraitError::unsupported(format!(
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
        let rel = self.check(rel)?;
        match &rel.rel_type {
            Some(RelType::Read(r)) => self.read(r),
            Some(RelType::Filter(f)) => self.filter(f),
            Some(RelType::Fetch(f)) => self.fetch(f),
            Some(RelType::Aggregate(a)) => self.aggregate(a),
            Some(RelType::Sort(s)) => self.sort(s),
            Some(RelType::Project(p)) => self.project(p),
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
        let fields = node.row_type().get_field_list().to_vec();
        let mut exprs = Vec::with_capacity(indices.len());
        for i in indices {
            let index = usize::try_from(i).map_err(|_| {
                SubstraitError::index_out_of_bounds(format!("Index {i} out of bounds for length {}", fields.len()))
            })?;
            let f = fields.get(index).ok_or_else(|| {
                SubstraitError::index_out_of_bounds(format!(
                    "Index {index} out of bounds for length {}",
                    fields.len()
                ))
            })?;
            exprs.push(self.rex_builder.make_input_ref(index, f.get_type().clone()));
        }
        Ok(self.rel_builder.project(node, exprs))
    }

    fn read(&self, r: &ReadRel) -> Result<RelNode> {
        let schema = r.base_schema.clone().unwrap_or_default();
        let st = schema.r#struct.clone().unwrap_or_default();
        let row_type = named_struct_to_calcite(self.system, &schema.names, &st.types, is_nullable(st.nullability))?;
        let node = match &r.read_type {
            Some(ReadType::NamedTable(t)) => self.rel_builder.scan(t.names.clone(), row_type),
            Some(ReadType::VirtualTable(v)) if v.values.is_empty() => {
                self.rel_builder.values(row_type, Vec::new())
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
                return Ok(self.rel_builder.values(row_type, tuples));
            }
            Some(ReadType::LocalFiles(_)) => {
                return Err(SubstraitError::unsupported(
                    "Rel LocalFiles of type io.substrait.relation.LocalFiles not handled by visitor type io.substrait.isthmus.SubstraitRelNodeConverter.",
                ))
            }
            Some(ReadType::ExtensionTable(_)) | Some(ReadType::IcebergTable(_)) => {
                return Err(SubstraitError::unsupported(
                    "Rel ExtensionTable of type io.substrait.relation.ExtensionTable not handled by visitor type io.substrait.isthmus.SubstraitRelNodeConverter.",
                ))
            }
            None => self.rel_builder.values(row_type, Vec::new()),
        };
        self.apply_remap(node, remap_of(r.common.as_ref()))
    }

    fn filter(&self, f: &FilterRel) -> Result<RelNode> {
        let input = self.convert(input_of(f.input.as_deref())?)?;
        let condition = f
            .condition
            .as_deref()
            .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
        let rex = self.expressions().convert(condition, input.row_type())?;
        let node = self.rel_builder.filter(input, vec![rex]);
        self.apply_remap(node, remap_of(f.common.as_ref()))
    }

    fn project(&self, p: &ProjectRel) -> Result<RelNode> {
        let child = self.convert(input_of(p.input.as_deref())?)?;
        let row_type = child.row_type().clone();
        let mut exprs: Vec<RexNode> = row_type
            .get_field_list()
            .iter()
            .enumerate()
            .map(|(i, f)| self.rex_builder.make_input_ref(i, f.get_type().clone()))
            .collect();
        let ex = self.expressions();
        for e in &p.expressions {
            exprs.push(ex.convert(e, &row_type)?);
        }
        let node = self.rel_builder.project_named(child, exprs, Vec::new(), false);
        self.apply_remap(node, remap_of(p.common.as_ref()))
    }

    fn fetch(&self, f: &FetchRel) -> Result<RelNode> {
        let child = self.convert(input_of(f.input.as_deref())?)?;
        let offset = match &f.offset_mode {
            Some(OffsetMode::Offset(o)) => *o,
            _ => 0,
        };
        let raw_count = match &f.count_mode {
            Some(CountMode::Count(c)) => *c,
            _ => 0,
        };
        let count = if raw_count != -1 { raw_count } else { -1 };
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
        let offset = i64::from(offset as i32);
        let count = i64::from(count as i32);
        let node = self.rel_builder.sort_limit(child, Some(offset), Some(count), Vec::new());
        self.apply_remap(node, remap_of(f.common.as_ref()))
    }

    fn sort(&self, s: &SortRel) -> Result<RelNode> {
        let child = self.convert(input_of(s.input.as_deref())?)?;
        let ex = self.expressions();
        let mut keys = Vec::with_capacity(s.sorts.len());
        for field in &s.sorts {
            let expr = field
                .expr
                .as_ref()
                .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
            let rex = ex.convert(expr, child.row_type())?;
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
                    reference_type: Some(reference_segment::ReferenceType::StructField(Box::new(StructField {
                        field: offset as i32,
                        child: None,
                    }))),
                })),
                root_type: Some(RootType::RootReference(RootReference {})),
            }))),
        }
    }

    fn aggregate(&self, a: &AggregateRel) -> Result<RelNode> {
        let child = self.convert(input_of(a.input.as_deref())?)?;
        let groupings: Vec<Vec<Expression>> = a.groupings.iter().map(|g| g.grouping_expressions.clone()).collect();
        let mut measures = a.measures.clone();
        let valid = measures.iter().all(|m| {
            m.measure.as_ref().is_none_or(|f| {
                f.arguments.iter().all(|arg| match &arg.arg_type {
                    Some(crate::proto::function_argument::ArgType::Value(e)) => Self::is_simple_field_reference(e),
                    _ => false,
                }) && f
                    .sorts
                    .iter()
                    .all(|s| s.expr.as_ref().is_some_and(Self::is_simple_field_reference))
            }) && m.filter.as_ref().is_none_or(Self::is_simple_field_reference)
        }) && groupings.iter().all(|g| {
            g.iter().all(Self::is_simple_field_reference) && {
                let offsets: Vec<i32> = g.iter().filter_map(Self::field_offset).collect();
                offsets.windows(2).all(|w| w[0] <= w[1])
            }
        });
        let (child, groupings) = if valid {
            (child, groupings)
        } else {
            let child_type = child.row_type().clone();
            let mut new_expressions: Vec<RexNode> = Vec::new();
            let mut offset = child_type.get_field_count();
            let ex = self.expressions();
            let mut project_out = |e: &Expression, new_expressions: &mut Vec<RexNode>| -> Result<Expression> {
                new_expressions.push(ex.convert(e, &child_type)?);
                let r = Self::field_ref_expression(offset);
                offset += 1;
                Ok(r)
            };
            for m in measures.iter_mut() {
                if let Some(f) = m.measure.as_mut() {
                    for arg in f.arguments.iter_mut() {
                        match &arg.arg_type {
                            Some(crate::proto::function_argument::ArgType::Value(e)) => {
                                if !Self::is_simple_field_reference(e) {
                                    let r = project_out(e, &mut new_expressions)?;
                                    arg.arg_type = Some(crate::proto::function_argument::ArgType::Value(r));
                                }
                            }
                            _ => {
                                return Err(SubstraitError::illegal_argument(
                                    "cannot handle non-expression argument for aggregate",
                                ))
                            }
                        }
                    }
                    for s in f.sorts.iter_mut() {
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
            for g in &groupings {
                let mut list = Vec::with_capacity(g.len());
                for e in g {
                    list.push(project_out(e, &mut new_expressions)?);
                }
                new_groupings.push(list);
            }
            let mut exprs: Vec<RexNode> = child_type
                .get_field_list()
                .iter()
                .enumerate()
                .map(|(i, f)| self.rex_builder.make_input_ref(i, f.get_type().clone()))
                .collect();
            exprs.extend(new_expressions);
            let projected = self.rel_builder.project_named(child, exprs, Vec::new(), false);
            (projected, new_groupings)
        };
        let row_type = child.row_type().clone();
        let ex = self.expressions();
        if groupings.len() > 1 {
            return Err(SubstraitError::unsupported(
                "Aggregates with more than one grouping set are not supported by the Rust Substrait converter",
            ));
        }
        let mut group_exprs = Vec::new();
        for g in &groupings {
            for e in g {
                group_exprs.push(ex.convert(e, &row_type)?);
            }
        }
        let mut calls = Vec::with_capacity(measures.len());
        for m in &measures {
            let f = m
                .measure
                .as_ref()
                .ok_or_else(|| SubstraitError::illegal_argument("Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan."))?;
            let (decl, op) = ex.aggregate_operator_for(f.function_reference)?;
            let args = ex.convert_arguments(decl, &f.arguments, &row_type)?;
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
                Some(e) => match ex.convert(e, &row_type)?.input_ref_index() {
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
                    .convert(e, &row_type)?
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
            calls.push(AggregateCall {
                op,
                distinct,
                approximate: false,
                args: arg_index,
                filter_arg,
                collation,
                ty: return_type,
                name: None,
            });
        }
        let node = self.rel_builder.aggregate(child, group_exprs, calls);
        self.apply_remap(node, remap_of(a.common.as_ref()))
    }
}
