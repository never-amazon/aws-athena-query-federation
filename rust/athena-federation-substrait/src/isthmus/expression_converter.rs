use std::collections::HashMap;

use bigdecimal::BigDecimal;

use super::function_mappings::{aggregate_operator, scalar_operator, window_operator};
use super::type_converter::{to_calcite, type_string};
use crate::calcite::operators::{self, SqlOperator};
use crate::calcite::rel::RelNode;
use crate::calcite::rel_data_type::{RelDataType, RelDataTypeSystem};
use crate::calcite::rex::{
    compose_disjunction, Direction, LiteralValue, NullDirection, RexBuilder, RexFieldCollation,
    RexLiteral, RexNode, RexOver, RexSubQuery, RexWindow, RexWindowBound, SubQueryKind, Symbol,
    TimeUnit, TrimFlag,
};
use crate::calcite::sarg::{Range, RangeSet, RexUnknownAs, Sarg};
use crate::calcite::sql_type_name::{SqlTypeFamily, SqlTypeName};
use crate::calcite::values::{BitString, DateString, TimeString, TimestampString};
use crate::error::{Result, SubstraitError};
use crate::java_cast::{int_to_byte, int_to_short, long_to_int};
use crate::literal::big_decimal_from_bytes;
use crate::proto::expression::field_reference::{ReferenceType, RootType};
use crate::proto::expression::literal::LiteralType;
use crate::proto::expression::reference_segment;
use crate::proto::expression::window_function::bound;
use crate::proto::expression::{
    window_function, FieldReference, Literal, RexType, ScalarFunction, WindowFunction,
};
use crate::proto::function_argument::ArgType;
use crate::proto::sort_field::{SortDirection, SortKind};
use crate::proto::{Expression, FunctionArgument, SortField, Type};

pub(crate) struct ExpressionConverter<'a> {
    pub(crate) system: RelDataTypeSystem,
    pub(crate) rex_builder: RexBuilder,
    pub(crate) functions: &'a HashMap<u32, String>,
    pub(crate) rels: &'a super::SubstraitToCalcite,
}

fn required<'b, T>(v: Option<&'b T>, what: &str) -> Result<&'b T> {
    v.ok_or_else(|| SubstraitError::IllegalArgument(format!("Unknown type: {what}")))
}

pub(crate) fn sort_direction(field: &SortField) -> Result<Option<(Direction, NullDirection)>> {
    let raw = match field.sort_kind {
        Some(SortKind::Direction(d)) => d,
        _ => 0,
    };
    match SortDirection::try_from(raw) {
        Ok(SortDirection::AscNullsFirst) => Ok(Some((Direction::Ascending, NullDirection::First))),
        Ok(SortDirection::AscNullsLast) => Ok(Some((Direction::Ascending, NullDirection::Last))),
        Ok(SortDirection::DescNullsFirst) => {
            Ok(Some((Direction::Descending, NullDirection::First)))
        }
        Ok(SortDirection::DescNullsLast) => Ok(Some((Direction::Descending, NullDirection::Last))),
        Ok(SortDirection::Clustered) => Ok(None),
        Ok(d @ SortDirection::Unspecified) => Err(SubstraitError::IllegalArgument(format!(
            "Unknown type: {}",
            d.as_str_name()
        ))),
        Err(_) => Err(SubstraitError::IllegalArgument(
            "Unknown type: UNRECOGNIZED".into(),
        )),
    }
}

fn time_unit(name: &str) -> Result<TimeUnit> {
    let unit = match name {
        "YEAR" => TimeUnit::Year,
        "QUARTER" => TimeUnit::Quarter,
        "MONTH" => TimeUnit::Month,
        "WEEK" => TimeUnit::Week,
        "DAY" => TimeUnit::Day,
        "HOUR" => TimeUnit::Hour,
        "MINUTE" => TimeUnit::Minute,
        "SECOND" => TimeUnit::Second,
        "MILLISECOND" => TimeUnit::Millisecond,
        "MICROSECOND" => TimeUnit::Microsecond,
        "NANOSECOND" => TimeUnit::Nanosecond,
        "DOW" => TimeUnit::Dow,
        "DOY" => TimeUnit::Doy,
        "EPOCH" => TimeUnit::Epoch,
        other => {
            return Err(SubstraitError::IllegalArgument(format!(
                "No enum constant org.apache.calcite.avatica.util.TimeUnitRange.{other}"
            )))
        }
    };
    Ok(unit)
}

fn trim_flag(name: &str) -> Result<TrimFlag> {
    match name {
        "BOTH" => Ok(TrimFlag::Both),
        "LEADING" => Ok(TrimFlag::Leading),
        "TRAILING" => Ok(TrimFlag::Trailing),
        other => Err(SubstraitError::IllegalArgument(format!(
            "No enum constant org.apache.calcite.sql.fun.SqlTrimFunction.Flag.{other}"
        ))),
    }
}

fn decimal_literal(
    s: RelDataTypeSystem,
    d: &crate::proto::expression::literal::Decimal,
) -> Result<RexNode> {
    if d.value.len() != 16 {
        return Err(SubstraitError::IllegalArgument(
            "Decimal value must be 16 bytes".into(),
        ));
    }
    let value = big_decimal_from_bytes(&d.value, i64::from(d.scale));
    let ty = RelDataType::create_sql_type_with_scale(s, SqlTypeName::Decimal, d.precision, d.scale);
    Ok(ExpressionConverter::lit(LiteralValue::Exact(value), ty))
}

fn empty_list_literal(s: RelDataTypeSystem, l: &crate::proto::r#type::List) -> Result<RexNode> {
    let element = l
        .r#type
        .as_deref()
        .ok_or_else(|| SubstraitError::Unsupported("Unsupported type: KIND_NOT_SET".into()))?;
    let ty = RelDataType::array(
        to_calcite(s, element)?,
        super::type_converter::is_nullable(l.nullability),
    );
    Ok(RexBuilder::make_call_typed(
        ty,
        &operators::ARRAY_VALUE_CONSTRUCTOR,
        Vec::new(),
    ))
}

fn precision_timestamp_literal(
    s: RelDataTypeSystem,
    type_name: SqlTypeName,
    label: &str,
    value: i64,
    precision: i32,
) -> Result<RexNode> {
    let max = s.max_precision(type_name);
    if precision > max {
        return Err(SubstraitError::Unsupported(format!(
            "unsupported {label} precision {precision}, max precision in Calcite type system is set to {max}"
        )));
    }
    let value = timestamp_string(value, precision)?;
    let ty = RelDataType::create_sql_type_with_precision(s, type_name, precision);
    Ok(ExpressionConverter::lit(LiteralValue::Timestamp(value), ty))
}

fn time_literal(s: RelDataTypeSystem, micros: i64) -> RexNode {
    let seconds = micros / 1_000_000;
    let nanos = long_to_int((micros - seconds * 1_000_000) * 1_000).cast_unsigned();
    let ts = TimeString::from_millis_of_day(long_to_int(seconds * 1000)).with_nanos(nanos);
    let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Time, 6);
    ExpressionConverter::lit(LiteralValue::Time(ts), ty)
}

#[allow(
    deprecated,
    reason = "reads deprecated Substrait proto fields that the Java converter still reads"
)]
fn interval_day_to_second_literal(
    id: &crate::proto::expression::literal::IntervalDayToSecond,
) -> RexNode {
    use crate::proto::expression::literal::interval_day_to_second::PrecisionMode;
    let (precision, subseconds) = match id.precision_mode {
        Some(PrecisionMode::Precision(p)) => (p, id.subseconds),
        Some(PrecisionMode::Microseconds(m)) => (6, i64::from(m)),
        None => (6, 0),
    };
    let millis = if precision > 3 {
        subseconds / 10i64.pow((precision - 3).unsigned_abs())
    } else {
        subseconds * 10i64.pow((3 - precision).unsigned_abs())
    };
    let total = i64::from(id.days) * 86_400_000 + i64::from(id.seconds) * 1000 + millis;
    ExpressionConverter::lit(
        LiteralValue::Interval(BigDecimal::from(total)),
        RelDataType::interval(SqlTypeName::IntervalDaySecond, 3),
    )
}

fn char_length(value: &str) -> i32 {
    i32::try_from(value.encode_utf16().count()).unwrap_or(i32::MAX)
}

fn millis_to_seconds_split(value: i64, unit_per_second: i64, nanos_per_unit: i64) -> (i64, u32) {
    let seconds = value.div_euclid(unit_per_second);
    let frac = value.rem_euclid(unit_per_second) * nanos_per_unit;
    (seconds, long_to_int(frac).cast_unsigned())
}

fn civil_from_days(epoch_day: i64) -> (i64, i64, i64) {
    let z = epoch_day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn check_year(kind: &str, epoch_day: i64) -> Result<()> {
    let (year, month, day) = civil_from_days(epoch_day);
    if (1..=9999).contains(&year) {
        return Ok(());
    }
    let year_text = if year > 9999 {
        format!("+{year}")
    } else if year < 0 {
        format!("-{:04}", -year)
    } else {
        format!("{year:04}")
    };
    Err(SubstraitError::IllegalArgument(format!(
        "{kind}{year_text}-{month:02}-{day:02} is outside the years 0001 to 9999"
    )))
}

fn timestamp_string(value: i64, precision: i32) -> Result<TimestampString> {
    let (seconds, nanos) = match precision {
        0 => (value, 0),
        3 => millis_to_seconds_split(value, 1_000, 1_000_000),
        6 => millis_to_seconds_split(value, 1_000_000, 1_000),
        9 => millis_to_seconds_split(value, 1_000_000_000, 1),
        other => {
            return Err(SubstraitError::Unsupported(format!(
                "Cannot handle PrecisionTimestamp with precision {other}."
            )))
        }
    };
    check_year("Timestamp literal on ", seconds.div_euclid(86_400))?;
    Ok(TimestampString::from_millis_since_epoch(seconds * 1000).with_nanos(nanos))
}

impl ExpressionConverter<'_> {
    fn ty(&self, t: Option<&Type>) -> Result<RelDataType> {
        let t =
            t.ok_or_else(|| SubstraitError::Unsupported("Unsupported type: KIND_NOT_SET".into()))?;
        to_calcite(self.system, t)
    }

    fn lit(value: LiteralValue, ty: RelDataType) -> RexNode {
        RexNode::Literal(RexLiteral { value, ty })
    }

    fn not_null(&self, name: SqlTypeName) -> RelDataType {
        RelDataType::create_sql_type(self.system, name)
    }

    pub(crate) fn convert(&self, expr: &Expression, input: &RelDataType) -> Result<RexNode> {
        let rex = expr.rex_type.as_ref().ok_or_else(|| {
            SubstraitError::IllegalArgument("Unknown type: REXTYPE_NOT_SET".into())
        })?;
        match rex {
            RexType::Literal(l) => self.literal(l),
            RexType::Selection(f) => self.field_reference(f, input),
            RexType::ScalarFunction(f) => self.scalar_function(f, input),
            RexType::WindowFunction(w) => self.window_function(w, input),
            RexType::IfThen(it) => {
                let mut args = Vec::new();
                for clause in &it.ifs {
                    args.push(self.convert(required(clause.r#if.as_ref(), "IF")?, input)?);
                    args.push(self.convert(required(clause.then.as_ref(), "THEN")?, input)?);
                }
                args.push(self.convert(required(it.r#else.as_deref(), "ELSE")?, input)?);
                let ty = case_type(&args);
                Ok(RexBuilder::make_call_typed(ty, &operators::CASE, args))
            }
            RexType::SwitchExpression(sw) => {
                let m = self.convert(required(sw.r#match.as_deref(), "MATCH")?, input)?;
                let mut args = Vec::new();
                for clause in &sw.ifs {
                    let cond = self.literal(required(clause.r#if.as_ref(), "IF")?)?;
                    args.push(self.rex_builder.make_call(&operators::EQUALS, vec![m.clone(), cond]));
                    args.push(self.convert(required(clause.then.as_ref(), "THEN")?, input)?);
                }
                args.push(self.convert(required(sw.r#else.as_deref(), "ELSE")?, input)?);
                let ty = case_type(&args);
                Ok(RexBuilder::make_call_typed(ty, &operators::CASE, args))
            }
            RexType::SingularOrList(or) => {
                let lhs = self.convert(required(or.value.as_deref(), "VALUE")?, input)?;
                let mut options = Vec::with_capacity(or.options.len());
                for o in &or.options {
                    options.push(self.convert(o, input)?);
                }
                Ok(self.make_in(lhs, options))
            }
            RexType::Cast(c) => {
                let ty = self.ty(c.r#type.as_ref())?;
                let operand = self.convert(required(c.input.as_deref(), "INPUT")?, input)?;
                let safe = c.failure_behavior == crate::proto::expression::cast::FailureBehavior::ReturnNull as i32;
                Ok(RexBuilder::make_cast(ty, operand, safe))
            }
            RexType::MultiOrList(_) => Err(SubstraitError::Unsupported("Expression MultiOrList not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.".into())),
            RexType::Subquery(q) => self.subquery(q, input),
            RexType::Nested(_) => Err(SubstraitError::IllegalArgument("Unknown type: NESTED".into())),
            RexType::DynamicParameter(_) => Err(SubstraitError::IllegalArgument("Unknown type: DYNAMIC_PARAMETER".into())),
            #[allow(deprecated, reason = "reads deprecated Substrait proto fields that the Java converter still reads")]
            RexType::Enum(_) => Err(SubstraitError::Unsupported("Unsupported type: ENUM".into())),
        }
    }

    fn subquery(
        &self,
        q: &crate::proto::expression::Subquery,
        input: &RelDataType,
    ) -> Result<RexNode> {
        use crate::proto::expression::subquery::set_predicate::PredicateOp;
        use crate::proto::expression::subquery::SubqueryType;
        let rel_of = |r: Option<&crate::proto::Rel>| -> Result<RelNode> {
            let r = r.ok_or_else(|| {
                SubstraitError::IllegalArgument("Unsupported RelTypeCase of RELTYPE_NOT_SET".into())
            })?;
            self.rels.convert_sub_query(r, input)
        };
        let boolean = |nullable: bool| self.rex_builder.boolean_type(nullable);
        let node = match q
            .subquery_type
            .as_ref()
            .ok_or_else(|| SubstraitError::IllegalArgument("Unknown type: SUBQUERYTYPE_NOT_SET".into()))?
        {
            SubqueryType::Scalar(s) => {
                let rel = rel_of(s.input.as_deref())?;
                let ty = rel
                    .row_type()
                    .field_list()
                    .first()
                    .map(|f| f.ty().with_nullability(true))
                    .ok_or_else(|| SubstraitError::IllegalArgument("scalar sub-query must return one column".into()))?;
                RexSubQuery {
                    kind: SubQueryKind::Scalar,
                    operands: Vec::new(),
                    rel,
                    ty,
                }
            }
            SubqueryType::InPredicate(p) => {
                let mut operands = Vec::with_capacity(p.needles.len());
                for n in &p.needles {
                    operands.push(self.convert(n, input)?);
                }
                let rel = rel_of(p.haystack.as_deref())?;
                let nullable = operands.iter().any(|o| o.ty().is_nullable())
                    || rel.row_type().field_list().iter().any(|f| f.ty().is_nullable());
                RexSubQuery {
                    kind: SubQueryKind::In,
                    operands,
                    rel,
                    ty: boolean(nullable),
                }
            }
            SubqueryType::SetPredicate(p) => {
                let kind = match PredicateOp::try_from(p.predicate_op) {
                    Ok(PredicateOp::Exists) => SubQueryKind::Exists,
                    Ok(PredicateOp::Unique) => SubQueryKind::Unique,
                    _ => {
                        return Err(SubstraitError::Unsupported(format!(
                            "Unsupported set predicate operation: {}",
                            p.predicate_op
                        )))
                    }
                };
                let rel = rel_of(p.tuples.as_deref())?;
                RexSubQuery {
                    kind,
                    operands: Vec::new(),
                    rel,
                    ty: boolean(false),
                }
            }
            SubqueryType::SetComparison(_) => {
                return Err(SubstraitError::Unsupported("Expression SetComparison of type io.substrait.expression.Expression not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.".into()))
            }
        };
        Ok(RexNode::SubQuery(Box::new(node)))
    }

    fn make_in(&self, lhs: RexNode, options: Vec<RexNode>) -> RexNode {
        let family = lhs.ty().sql_type_name().family();
        let assignable = options
            .iter()
            .all(|o| same_family(family, o.ty().sql_type_name().family()));
        if assignable {
            let mut values = Vec::new();
            for o in &options {
                match o {
                    RexNode::Literal(l) if !l.is_null() => {
                        if let Some(v) = l.value.to_sarg_value() {
                            values.push(v);
                        } else {
                            values.clear();
                            break;
                        }
                    }
                    _ => {
                        values.clear();
                        break;
                    }
                }
            }
            if values.len() == options.len() && !values.is_empty() {
                let mut rs = RangeSet::empty();
                for v in values {
                    rs.add(Range::singleton(v));
                }
                let sarg = Sarg::of(RexUnknownAs::Unknown, rs);
                if sarg.is_points() && sarg.point_count() == 1 {
                    return self
                        .rex_builder
                        .make_call(&operators::EQUALS, vec![lhs, options[0].clone()]);
                }
                let types: Vec<RelDataType> = options.iter().map(|o| o.ty().clone()).collect();
                let ty = least_restrictive(&types);
                return self.rex_builder.make_search(lhs, sarg, ty);
            }
        }
        let calls = options
            .into_iter()
            .map(|o| {
                self.rex_builder
                    .make_call(&operators::EQUALS, vec![lhs.clone(), o])
            })
            .collect();
        compose_disjunction(self.rex_builder, calls)
    }

    fn field_reference(&self, f: &FieldReference, input: &RelDataType) -> Result<RexNode> {
        let segment = match &f.reference_type {
            Some(ReferenceType::DirectReference(s)) => s,
            Some(ReferenceType::MaskedReference(_)) => {
                return Err(SubstraitError::IllegalArgument(
                    "Unsupported type: MASKED_REFERENCE".into(),
                ))
            }
            None => {
                return Err(SubstraitError::IllegalArgument(
                    "Unhandled type: REFERENCETYPE_NOT_SET".into(),
                ))
            }
        };
        match &f.root_type {
            Some(RootType::RootReference(_) | RootType::OuterReference(_)) => {}
            Some(RootType::Expression(_)) | None => {
                return Err(SubstraitError::Unsupported(format!(
                    "Expression {f:?} of type io.substrait.expression.FieldReference not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
                )))
            }
        }
        let field = match &segment.reference_type {
            Some(reference_segment::ReferenceType::StructField(sf)) if sf.child.is_none() => sf.field,
            Some(_) => {
                return Err(SubstraitError::Unsupported("Expression FieldReference with nested segments not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.".into()))
            }
            None => {
                return Err(SubstraitError::IllegalArgument("Unhandled type: REFERENCETYPE_NOT_SET".into()))
            }
        };
        if let Some(RootType::OuterReference(o)) = &f.root_type {
            let index = usize::try_from(field).map_err(|_| {
                SubstraitError::IndexOutOfBounds(format!("Index {field} out of bounds"))
            })?;
            return self.rels.outer_field(o.steps_out, index);
        }
        let fields = input.field_list();
        let index = usize::try_from(field).map_err(|_| {
            SubstraitError::IndexOutOfBounds(format!(
                "Index {field} out of bounds for length {}",
                fields.len()
            ))
        })?;
        let f = fields.get(index).ok_or_else(|| {
            SubstraitError::IndexOutOfBounds(format!(
                "Index {index} out of bounds for length {}",
                fields.len()
            ))
        })?;
        Ok(RexBuilder::make_input_ref(index, f.ty().clone()))
    }

    fn function_decl(&self, reference: u32) -> Result<&str> {
        self.functions.get(&reference).map(String::as_str).ok_or_else(|| {
            SubstraitError::IllegalArgument("Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan.".into())
        })
    }

    fn arguments(
        &self,
        decl: &str,
        args: &[FunctionArgument],
        input: &RelDataType,
        trim: Option<TrimFlag>,
    ) -> Result<Vec<RexNode>> {
        let mut out = Vec::new();
        let mut index = 0usize;
        if let Some(flag) = trim {
            out.push(Self::symbol(
                self.not_null(SqlTypeName::Symbol),
                Symbol::Trim(flag),
            ));
            index = 1;
        }
        for a in args {
            match &a.arg_type {
                Some(ArgType::Value(e)) => out.push(self.convert(e, input)?),
                Some(ArgType::Enum(v)) => out.push(self.enum_arg(decl, index, v)?),
                Some(ArgType::Type(t)) => {
                    return Err(SubstraitError::Unsupported(format!(
                        "FunctionArg {t:?} not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
                    )))
                }
                None => return Err(SubstraitError::IllegalArgument("Unknown type: ARGTYPE_NOT_SET".into())),
            }
            index += 1;
        }
        Ok(out)
    }

    fn symbol(ty: RelDataType, symbol: Symbol) -> RexNode {
        Self::lit(LiteralValue::Symbol(symbol), ty)
    }

    fn enum_arg(&self, decl: &str, index: usize, value: &str) -> Result<RexNode> {
        let not_handled = || {
            SubstraitError::Unsupported(format!(
                "EnumArg(value={value}) not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
            ))
        };
        if index != 0 {
            return Err(not_handled());
        }
        let symbol = match decl {
            "extract" => Symbol::TimeUnit(time_unit(value)?),
            "trim" | "ltrim" | "rtrim" => Symbol::Trim(trim_flag(value)?),
            _ => return Err(not_handled()),
        };
        Ok(Self::symbol(self.not_null(SqlTypeName::Symbol), symbol))
    }

    fn scalar_function(&self, f: &ScalarFunction, input: &RelDataType) -> Result<RexNode> {
        let decl = self.function_decl(f.function_reference)?;
        let output = f
            .output_type
            .as_ref()
            .ok_or_else(|| SubstraitError::Unsupported("Unsupported type: KIND_NOT_SET".into()))?;
        let return_type = to_calcite(self.system, output)?;
        let op = scalar_operator(decl, output.kind.as_ref()).ok_or_else(|| {
            SubstraitError::IllegalArgument(format!(
                "Unable to convert scalar function {}({}).",
                decl,
                Self::describe_args(&f.arguments)
            ))
        })?;
        let trim = match decl {
            "trim" => Some(TrimFlag::Both),
            "ltrim" => Some(TrimFlag::Leading),
            "rtrim" => Some(TrimFlag::Trailing),
            _ => None,
        };
        let args = self.arguments(decl, &f.arguments, input, trim)?;
        Ok(RexBuilder::make_call_typed(return_type, op, args))
    }

    fn describe_args(args: &[FunctionArgument]) -> String {
        args.iter()
            .map(|a| match &a.arg_type {
                Some(ArgType::Enum(v)) => v.clone(),
                Some(ArgType::Type(t)) => type_string(t).to_string(),
                Some(ArgType::Value(_)) | None => "expr".to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn window_function(&self, w: &WindowFunction, input: &RelDataType) -> Result<RexNode> {
        let decl = self.function_decl(w.function_reference)?;
        let op = window_operator(decl).ok_or_else(|| {
            SubstraitError::IllegalArgument(format!(
                "Unable to convert window function {}({}).",
                decl,
                Self::describe_args(&w.arguments)
            ))
        })?;
        let output_type = self.ty(w.output_type.as_ref())?;
        let args = self.arguments(decl, &w.arguments, input, None)?;
        let mut partition_keys = Vec::with_capacity(w.partitions.len());
        for p in &w.partitions {
            partition_keys.push(self.convert(p, input)?);
        }
        let mut order_keys = Vec::with_capacity(w.sorts.len());
        for s in &w.sorts {
            let expr = self.convert(required(s.expr.as_ref(), "EXPR")?, input)?;
            let (direction, null_direction) = sort_direction(s)?.ok_or_else(|| {
                SubstraitError::IllegalArgument("SORT_DIRECTION_CLUSTERED is not supported".into())
            })?;
            order_keys.push(RexFieldCollation {
                expr,
                direction,
                null_direction,
            });
        }
        let lower_bound =
            self.window_bound(w.lower_bound.as_ref(), RexWindowBound::UnboundedPreceding);
        let upper_bound =
            self.window_bound(w.upper_bound.as_ref(), RexWindowBound::UnboundedFollowing);
        let is_rows = match window_function::BoundsType::try_from(w.bounds_type) {
            Ok(window_function::BoundsType::Rows) => true,
            Ok(window_function::BoundsType::Range) => false,
            Ok(window_function::BoundsType::Unspecified) => {
                return Err(SubstraitError::IllegalArgument(
                    "bounds type on window function must be specified".into(),
                ))
            }
            Err(_) => {
                return Err(SubstraitError::IllegalArgument(
                    "Unknown type: UNRECOGNIZED".into(),
                ))
            }
        };
        let distinct =
            match crate::proto::aggregate_function::AggregationInvocation::try_from(w.invocation) {
                Ok(crate::proto::aggregate_function::AggregationInvocation::Distinct) => true,
                Ok(_) => false,
                Err(_) => {
                    return Err(SubstraitError::IllegalArgument(
                        "Unknown type: UNRECOGNIZED".into(),
                    ))
                }
            };
        Ok(RexNode::Over(Box::new(RexOver {
            op,
            operands: args,
            ty: output_type,
            window: RexWindow {
                partition_keys,
                order_keys,
                lower_bound,
                upper_bound,
                is_rows,
            },
            distinct,
            ignore_nulls: false,
        })))
    }

    fn window_bound(
        &self,
        b: Option<&window_function::Bound>,
        unbounded: RexWindowBound,
    ) -> RexWindowBound {
        match b.and_then(|b| b.kind.as_ref()) {
            Some(bound::Kind::Preceding(p)) => {
                RexWindowBound::Preceding(self.rex_builder.make_bigint_literal(p.offset))
            }
            Some(bound::Kind::Following(f)) => {
                RexWindowBound::Following(self.rex_builder.make_bigint_literal(f.offset))
            }
            Some(bound::Kind::CurrentRow(_)) => RexWindowBound::CurrentRow,
            Some(bound::Kind::Unbounded(_)) | None => unbounded,
        }
    }

    pub(crate) fn aggregate_operator_for(
        &self,
        reference: u32,
    ) -> Result<(&str, &'static SqlOperator)> {
        let decl = self.function_decl(reference)?;
        let op = aggregate_operator(decl).ok_or_else(|| {
            SubstraitError::IllegalArgument(format!("Unable to find binding for call {decl}"))
        })?;
        Ok((decl, op))
    }

    pub(crate) fn convert_arguments(
        &self,
        decl: &str,
        args: &[FunctionArgument],
        input: &RelDataType,
    ) -> Result<Vec<RexNode>> {
        self.arguments(decl, args, input, None)
    }

    #[allow(
        deprecated,
        reason = "reads deprecated Substrait proto fields that the Java converter still reads"
    )]
    pub(crate) fn literal(&self, l: &Literal) -> Result<RexNode> {
        let nullable = l.nullable;
        let lt = l.literal_type.as_ref().ok_or_else(|| {
            SubstraitError::IllegalState("Unexpected value: LITERALTYPE_NOT_SET".into())
        })?;
        let s = self.system;
        let exact = |v: i64, name: SqlTypeName| -> RexNode {
            Self::lit(
                LiteralValue::Exact(BigDecimal::from(v)),
                RelDataType::create_sql_type(s, name),
            )
        };
        let node = match lt {
            LiteralType::Boolean(b) => Self::lit(LiteralValue::Boolean(*b), self.not_null(SqlTypeName::Boolean)),
            LiteralType::I8(v) => exact(i64::from(int_to_byte(*v)), SqlTypeName::Tinyint),
            LiteralType::I16(v) => exact(i64::from(int_to_short(*v)), SqlTypeName::Smallint),
            LiteralType::I32(v) => exact(i64::from(*v), SqlTypeName::Integer),
            LiteralType::I64(v) => exact(*v, SqlTypeName::Bigint),
            LiteralType::Fp32(v) => Self::lit(LiteralValue::Approx(f64::from(*v)), self.not_null(SqlTypeName::Real)),
            LiteralType::Fp64(v) => Self::lit(LiteralValue::Approx(*v), self.not_null(SqlTypeName::Double)),
            LiteralType::String(v) => self.string_literal(v, RelDataType::create_sql_type(s, SqlTypeName::Varchar), nullable),
            LiteralType::VarChar(v) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Varchar, v.length.cast_signed());
                self.string_literal(&v.value, ty, nullable)
            }
            LiteralType::FixedChar(v) => {
                let len = char_length(v);
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Char, len);
                Self::lit(LiteralValue::Char(v.clone()), ty)
            }
            LiteralType::Binary(v) => {
                let ty = RelDataType::create_sql_type(s, SqlTypeName::Varbinary);
                Self::binary_literal(v, ty, nullable)
            }
            LiteralType::FixedBinary(v) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Binary, i32::try_from(v.len()).unwrap_or(i32::MAX));
                Self::lit(LiteralValue::Binary(BitString::create_from_bytes(v)), ty)
            }
            LiteralType::Date(d) => {
                check_year("Date literal ", i64::from(*d))?;
                Self::lit(
                    LiteralValue::Date(DateString::from_days_since_epoch(*d)),
                    self.not_null(SqlTypeName::Date),
                )
            }
            LiteralType::Time(micros) => time_literal(s, *micros),
            LiteralType::Timestamp(micros) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Timestamp, 6);
                Self::lit(LiteralValue::Timestamp(timestamp_string(*micros, 6)?), ty)
            }
            LiteralType::TimestampTz(micros) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::TimestampWithLocalTimeZone, 6);
                Self::lit(LiteralValue::Timestamp(timestamp_string(*micros, 6)?), ty)
            }
            LiteralType::PrecisionTimestamp(p) => precision_timestamp_literal(
                s,
                SqlTypeName::Timestamp,
                "precision_timestamp",
                p.value,
                p.precision,
            )?,
            LiteralType::PrecisionTimestampTz(p) => precision_timestamp_literal(
                s,
                SqlTypeName::TimestampWithLocalTimeZone,
                "precision_timestamp_tz",
                p.value,
                p.precision,
            )?,
            LiteralType::Decimal(d) => decimal_literal(s, d)?,
            LiteralType::IntervalYearToMonth(iy) => {
                let months = i64::from(iy.years) * 12 + i64::from(iy.months);
                Self::lit(
                    LiteralValue::Interval(BigDecimal::from(months)),
                    RelDataType::interval(SqlTypeName::IntervalYearMonth, crate::calcite::rel_data_type::PRECISION_NOT_SPECIFIED),
                )
            }
            LiteralType::IntervalDayToSecond(id) => interval_day_to_second_literal(id),
            LiteralType::Null(t) => {
                let ty = to_calcite(s, t)?;
                RexBuilder::make_null_literal(&ty)
            }
            LiteralType::List(list) => self.list_literal(list)?,
            LiteralType::EmptyList(l) => empty_list_literal(s, l)?,
            other => {
                return Err(SubstraitError::Unsupported(format!(
                    "Expression {} of type io.substrait.expression.Expression not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.",
                    crate::literal::literal_type_case_name(other)
                )))
            }
        };
        Ok(node)
    }

    fn list_literal(&self, list: &crate::proto::expression::literal::List) -> Result<RexNode> {
        let mut args = Vec::with_capacity(list.values.len());
        for v in &list.values {
            args.push(self.literal(v)?);
        }
        let component = args
            .first()
            .map_or_else(|| self.not_null(SqlTypeName::Null), |a| a.ty().clone());
        let ty = RelDataType::array(least_restrictive_or(&args, component), false);
        Ok(RexBuilder::make_call_typed(
            ty,
            &operators::ARRAY_VALUE_CONSTRUCTOR,
            args,
        ))
    }

    fn string_literal(&self, value: &str, ty: RelDataType, nullable: bool) -> RexNode {
        let len = char_length(value);
        let precision = ty.precision();
        let nullable_ty = nullable.then(|| ty.with_nullability(true));
        let literal = if precision < 0 || precision >= len {
            Self::lit(LiteralValue::Char(value.to_string()), ty)
        } else {
            let char_ty = RelDataType::create_sql_type_with_precision(
                self.system,
                SqlTypeName::Char,
                len.max(1),
            );
            let inner = Self::lit(LiteralValue::Char(value.to_string()), char_ty);
            RexBuilder::make_cast(ty, inner, false)
        };
        match nullable_ty {
            Some(t) => RexBuilder::make_cast(t, literal, false),
            None => literal,
        }
    }

    fn binary_literal(value: &[u8], ty: RelDataType, nullable: bool) -> RexNode {
        let nullable_ty = nullable.then(|| ty.with_nullability(true));
        let literal = Self::lit(
            LiteralValue::Binary(BitString::create_from_bytes(value)),
            ty,
        );
        match nullable_ty {
            Some(t) => RexBuilder::make_cast(t, literal, false),
            None => literal,
        }
    }
}

fn same_family(a: SqlTypeFamily, b: SqlTypeFamily) -> bool {
    a == b
}

fn least_restrictive_or(args: &[RexNode], fallback: RelDataType) -> RelDataType {
    if args.is_empty() {
        return fallback;
    }
    let types: Vec<RelDataType> = args.iter().map(|a| a.ty().clone()).collect();
    least_restrictive(&types)
}

pub(crate) fn least_restrictive(types: &[RelDataType]) -> RelDataType {
    let nullable = types.iter().any(RelDataType::is_nullable);
    let non_null: Vec<&RelDataType> = types
        .iter()
        .filter(|t| t.sql_type_name() != SqlTypeName::Null)
        .collect();
    let Some(first) = non_null.first() else {
        return types
            .first()
            .cloned()
            .unwrap_or_else(|| RelDataType::basic(SqlTypeName::Null, -1, i32::MIN, true))
            .with_nullability(true);
    };
    let mut best: RelDataType = (*first).clone();
    for t in non_null.iter().skip(1) {
        best = merge_types(&best, t);
    }
    best.with_nullability(nullable)
}

fn merge_types(a: &RelDataType, b: &RelDataType) -> RelDataType {
    let an = a.sql_type_name();
    let bn = b.sql_type_name();
    if an.is_character() && bn.is_character() {
        let varchar = an == SqlTypeName::Varchar || bn == SqlTypeName::Varchar;
        let pa = a.precision();
        let pb = b.precision();
        let precision = if pa < 0 || pb < 0 { -1 } else { pa.max(pb) };
        let name = if varchar {
            SqlTypeName::Varchar
        } else {
            SqlTypeName::Char
        };
        return RelDataType::basic(name, precision, i32::MIN, false);
    }
    if an == bn {
        if a.precision() >= b.precision() {
            return a.clone();
        }
        return b.clone();
    }
    if an.is_approx() || bn.is_approx() {
        return RelDataType::basic(SqlTypeName::Double, -1, i32::MIN, false);
    }
    let rank = |n: SqlTypeName| match n {
        SqlTypeName::Tinyint => 1,
        SqlTypeName::Smallint => 2,
        SqlTypeName::Integer => 3,
        SqlTypeName::Bigint => 4,
        SqlTypeName::Decimal => 5,
        _ => 0,
    };
    if rank(an) >= rank(bn) {
        a.clone()
    } else {
        b.clone()
    }
}

pub(crate) fn case_type(args: &[RexNode]) -> RelDataType {
    let n = args.len();
    let mut values: Vec<RelDataType> = Vec::new();
    let mut i = 1;
    while i < n {
        values.push(args[i].ty().clone());
        i += 2;
    }
    if let Some(last) = args.last() {
        values.push(last.ty().clone());
    }
    least_restrictive(&values)
}
