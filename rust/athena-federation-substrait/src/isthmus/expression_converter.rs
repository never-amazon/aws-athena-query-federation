use std::collections::HashMap;

use bigdecimal::BigDecimal;

use super::function_mappings::{aggregate_operator, scalar_operator, window_operator, FunctionDecl};
use super::type_converter::{to_calcite, type_string};
use crate::calcite::operators::{self, SqlOperator};
use crate::calcite::rel_data_type::{RelDataType, RelDataTypeSystem};
use crate::calcite::rex::{
    compose_disjunction, Direction, LiteralValue, NullDirection, RexBuilder, RexFieldCollation, RexLiteral,
    RexNode, RexOver, RexWindow, RexWindowBound, Symbol, TimeUnit, TrimFlag,
};
use crate::calcite::sarg::{Range, RangeSet, RexUnknownAs, Sarg};
use crate::calcite::sql_type_name::{SqlTypeFamily, SqlTypeName};
use crate::calcite::values::{BitString, DateString, TimeString, TimestampString};
use crate::error::{Result, SubstraitError};
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
use crate::substrait_literal_converter::big_decimal_from_bytes;

pub(crate) struct ExpressionConverter<'a> {
    pub(crate) system: RelDataTypeSystem,
    pub(crate) rex_builder: RexBuilder,
    pub(crate) functions: &'a HashMap<u32, FunctionDecl>,
}

fn required<'b, T>(v: Option<&'b T>, what: &str) -> Result<&'b T> {
    v.ok_or_else(|| SubstraitError::illegal_argument(format!("Unknown type: {what}")))
}

pub(crate) fn sort_direction(field: &SortField) -> Result<SortDirection> {
    let raw = match field.sort_kind {
        Some(SortKind::Direction(d)) => d,
        _ => 0,
    };
    match SortDirection::try_from(raw) {
        Ok(SortDirection::Unspecified) | Err(_) => Err(SubstraitError::illegal_argument(format!(
            "Unknown type: {}",
            SortDirection::try_from(raw)
                .map(|d| d.as_str_name())
                .unwrap_or("UNRECOGNIZED")
        ))),
        Ok(d) => Ok(d),
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
            return Err(SubstraitError::illegal_argument(format!(
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
        other => Err(SubstraitError::illegal_argument(format!(
            "No enum constant org.apache.calcite.sql.fun.SqlTrimFunction.Flag.{other}"
        ))),
    }
}

fn millis_to_seconds_split(value: i64, unit_per_second: i64, nanos_per_unit: i64) -> (i64, u32) {
    let seconds = value / unit_per_second;
    let frac = (value - seconds * unit_per_second) * nanos_per_unit;
    (seconds, frac as u32)
}

fn timestamp_string(value: i64, precision: i32) -> Result<TimestampString> {
    let (seconds, nanos) = match precision {
        0 => (value, 0),
        3 => millis_to_seconds_split(value, 1_000, 1_000_000),
        6 => millis_to_seconds_split(value, 1_000_000, 1_000),
        9 => millis_to_seconds_split(value, 1_000_000_000, 1),
        other => {
            return Err(SubstraitError::unsupported(format!(
                "Cannot handle PrecisionTimestamp with precision {other}."
            )))
        }
    };
    Ok(TimestampString::from_millis_since_epoch(seconds * 1000).with_nanos(nanos))
}

impl<'a> ExpressionConverter<'a> {
    fn ty(&self, t: Option<&Type>) -> Result<RelDataType> {
        let t = t.ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
        to_calcite(self.system, t)
    }

    fn lit(&self, value: LiteralValue, ty: RelDataType) -> RexNode {
        RexNode::Literal(RexLiteral { value, ty })
    }

    fn not_null(&self, name: SqlTypeName) -> RelDataType {
        RelDataType::create_sql_type(self.system, name)
    }

    pub(crate) fn convert(&self, expr: &Expression, input: &RelDataType) -> Result<RexNode> {
        let rex = expr
            .rex_type
            .as_ref()
            .ok_or_else(|| SubstraitError::illegal_argument("Unknown type: REXTYPE_NOT_SET"))?;
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
                Ok(self.rex_builder.make_call_typed(ty, &operators::CASE, args))
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
                Ok(self.rex_builder.make_call_typed(ty, &operators::CASE, args))
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
                Ok(self.rex_builder.make_cast(ty, operand, safe))
            }
            RexType::MultiOrList(_) => Err(SubstraitError::unsupported(
                "Expression MultiOrList not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.",
            )),
            RexType::Subquery(_) => Err(SubstraitError::unsupported(
                "Subquery expressions are not supported by the Rust Substrait converter",
            )),
            RexType::Nested(_) => Err(SubstraitError::illegal_argument("Unknown type: NESTED")),
            RexType::DynamicParameter(_) => Err(SubstraitError::illegal_argument("Unknown type: DYNAMIC_PARAMETER")),
            #[allow(deprecated)]
            RexType::Enum(_) => Err(SubstraitError::unsupported("Unsupported type: ENUM")),
        }
    }

    fn make_in(&self, lhs: RexNode, options: Vec<RexNode>) -> RexNode {
        let family = lhs.get_type().get_sql_type_name().family();
        let assignable = options
            .iter()
            .all(|o| same_family(family, o.get_type().get_sql_type_name().family()));
        if assignable {
            let mut values = Vec::new();
            for o in &options {
                match o {
                    RexNode::Literal(l) if !l.is_null() => match l.value.to_sarg_value() {
                        Some(v) => values.push(v),
                        None => {
                            values.clear();
                            break;
                        }
                    },
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
                let types: Vec<RelDataType> = options.iter().map(|o| o.get_type().clone()).collect();
                let ty = least_restrictive(&types);
                return self.rex_builder.make_search(lhs, sarg, ty);
            }
        }
        let calls = options
            .into_iter()
            .map(|o| self.rex_builder.make_call(&operators::EQUALS, vec![lhs.clone(), o]))
            .collect();
        compose_disjunction(&self.rex_builder, calls)
    }

    fn field_reference(&self, f: &FieldReference, input: &RelDataType) -> Result<RexNode> {
        let segment = match &f.reference_type {
            Some(ReferenceType::DirectReference(s)) => s,
            Some(ReferenceType::MaskedReference(_)) => {
                return Err(SubstraitError::illegal_argument(
                    "Unsupported type: MASKED_REFERENCE",
                ))
            }
            None => {
                return Err(SubstraitError::illegal_argument(
                    "Unhandled type: REFERENCETYPE_NOT_SET",
                ))
            }
        };
        match &f.root_type {
            Some(RootType::RootReference(_)) => {}
            Some(RootType::OuterReference(_)) => {
                return Err(SubstraitError::unsupported(
                    "Outer references are not supported by the Rust Substrait converter",
                ))
            }
            Some(RootType::Expression(_)) | None => {
                return Err(SubstraitError::unsupported(format!(
                    "Expression {f:?} of type io.substrait.expression.FieldReference not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
                )))
            }
        }
        let field = match &segment.reference_type {
            Some(reference_segment::ReferenceType::StructField(sf)) if sf.child.is_none() => sf.field,
            Some(_) => {
                return Err(SubstraitError::unsupported(
                    "Expression FieldReference with nested segments not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.",
                ))
            }
            None => {
                return Err(SubstraitError::illegal_argument(
                    "Unhandled type: REFERENCETYPE_NOT_SET",
                ))
            }
        };
        let fields = input.get_field_list();
        let index = usize::try_from(field).map_err(|_| {
            SubstraitError::index_out_of_bounds(format!("Index {field} out of bounds for length {}", fields.len()))
        })?;
        let f = fields.get(index).ok_or_else(|| {
            SubstraitError::index_out_of_bounds(format!("Index {index} out of bounds for length {}", fields.len()))
        })?;
        Ok(self.rex_builder.make_input_ref(index, f.get_type().clone()))
    }

    fn function_decl(&self, reference: u32) -> Result<&FunctionDecl> {
        self.functions.get(&reference).ok_or_else(|| {
            SubstraitError::illegal_argument(
                "Unknown function id. Make sure that the function id provided was shared in the extensions section of the plan.",
            )
        })
    }

    fn arguments(
        &self,
        decl: &FunctionDecl,
        args: &[FunctionArgument],
        input: &RelDataType,
        extra_enum: Option<String>,
    ) -> Result<Vec<RexNode>> {
        let mut out = Vec::new();
        let mut index = 0usize;
        if let Some(flag) = extra_enum {
            out.push(self.enum_arg(decl, 0, &flag)?);
            index = 1;
        }
        for a in args {
            match &a.arg_type {
                Some(ArgType::Value(e)) => out.push(self.convert(e, input)?),
                Some(ArgType::Enum(v)) => out.push(self.enum_arg(decl, index, v)?),
                Some(ArgType::Type(t)) => {
                    return Err(SubstraitError::unsupported(format!(
                        "FunctionArg {t:?} not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
                    )))
                }
                None => return Err(SubstraitError::illegal_argument("Unknown type: ARGTYPE_NOT_SET")),
            }
            index += 1;
        }
        Ok(out)
    }

    fn enum_arg(&self, decl: &FunctionDecl, index: usize, value: &str) -> Result<RexNode> {
        let not_handled = || {
            SubstraitError::unsupported(format!(
                "EnumArg(value={value}) not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter."
            ))
        };
        if index != 0 {
            return Err(not_handled());
        }
        let symbol = match decl.name.as_str() {
            "extract" => Symbol::TimeUnit(time_unit(value)?),
            "trim" | "ltrim" | "rtrim" => Symbol::Trim(trim_flag(value)?),
            _ => return Err(not_handled()),
        };
        Ok(self.lit(
            LiteralValue::Symbol(symbol),
            self.not_null(SqlTypeName::Symbol),
        ))
    }

    fn scalar_function(&self, f: &ScalarFunction, input: &RelDataType) -> Result<RexNode> {
        let decl = self.function_decl(f.function_reference)?;
        let output = f
            .output_type
            .as_ref()
            .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
        let return_type = to_calcite(self.system, output)?;
        let op = scalar_operator(&decl.name, type_string(output)).ok_or_else(|| {
            SubstraitError::illegal_argument(format!(
                "Unable to convert scalar function {}({}).",
                decl.name,
                self.describe_args(&f.arguments)
            ))
        })?;
        let extra = match decl.name.as_str() {
            "trim" => Some("BOTH".to_string()),
            "ltrim" => Some("LEADING".to_string()),
            "rtrim" => Some("TRAILING".to_string()),
            _ => None,
        };
        let args = self.arguments(decl, &f.arguments, input, extra)?;
        Ok(self.rex_builder.make_call_typed(return_type, op, args))
    }

    fn describe_args(&self, args: &[FunctionArgument]) -> String {
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
        let op = window_operator(&decl.name).ok_or_else(|| {
            SubstraitError::illegal_argument(format!(
                "Unable to convert window function {}({}).",
                decl.name,
                self.describe_args(&w.arguments)
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
            let (direction, null_direction) = match sort_direction(s)? {
                SortDirection::AscNullsFirst => (Direction::Ascending, NullDirection::First),
                SortDirection::AscNullsLast => (Direction::Ascending, NullDirection::Last),
                SortDirection::DescNullsFirst => (Direction::Descending, NullDirection::First),
                SortDirection::DescNullsLast => (Direction::Descending, NullDirection::Last),
                _ => {
                    return Err(SubstraitError::illegal_argument(
                        "SORT_DIRECTION_CLUSTERED is not supported",
                    ))
                }
            };
            order_keys.push(RexFieldCollation {
                expr,
                direction,
                null_direction,
            });
        }
        let lower_bound = self.window_bound(w.lower_bound.as_ref(), RexWindowBound::UnboundedPreceding);
        let upper_bound = self.window_bound(w.upper_bound.as_ref(), RexWindowBound::UnboundedFollowing);
        let is_rows = match window_function::BoundsType::try_from(w.bounds_type) {
            Ok(window_function::BoundsType::Rows) => true,
            Ok(window_function::BoundsType::Range) => false,
            Ok(window_function::BoundsType::Unspecified) => {
                return Err(SubstraitError::illegal_argument(
                    "bounds type on window function must be specified",
                ))
            }
            Err(_) => return Err(SubstraitError::illegal_argument("Unknown type: UNRECOGNIZED")),
        };
        let distinct = match crate::proto::aggregate_function::AggregationInvocation::try_from(w.invocation) {
            Ok(crate::proto::aggregate_function::AggregationInvocation::Distinct) => true,
            Ok(_) => false,
            Err(_) => return Err(SubstraitError::illegal_argument("Unknown type: UNRECOGNIZED")),
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

    fn window_bound(&self, b: Option<&window_function::Bound>, unbounded: RexWindowBound) -> RexWindowBound {
        match b.and_then(|b| b.kind.as_ref()) {
            Some(bound::Kind::Preceding(p)) => RexWindowBound::Preceding(self.rex_builder.make_bigint_literal(p.offset)),
            Some(bound::Kind::Following(f)) => RexWindowBound::Following(self.rex_builder.make_bigint_literal(f.offset)),
            Some(bound::Kind::CurrentRow(_)) => RexWindowBound::CurrentRow,
            Some(bound::Kind::Unbounded(_)) | None => unbounded,
        }
    }

    pub(crate) fn aggregate_operator_for(&self, reference: u32) -> Result<(&FunctionDecl, &'static SqlOperator)> {
        let decl = self.function_decl(reference)?;
        let op = aggregate_operator(&decl.name).ok_or_else(|| {
            SubstraitError::illegal_argument(format!("Unable to find binding for call {}", decl.name))
        })?;
        Ok((decl, op))
    }

    pub(crate) fn convert_arguments(
        &self,
        decl: &FunctionDecl,
        args: &[FunctionArgument],
        input: &RelDataType,
    ) -> Result<Vec<RexNode>> {
        self.arguments(decl, args, input, None)
    }

    #[allow(deprecated)]
    pub(crate) fn literal(&self, l: &Literal) -> Result<RexNode> {
        let nullable = l.nullable;
        let lt = l
            .literal_type
            .as_ref()
            .ok_or_else(|| SubstraitError::illegal_state("Unexpected value: LITERALTYPE_NOT_SET"))?;
        let s = self.system;
        let exact = |v: i64, name: SqlTypeName| -> RexNode {
            self.lit(LiteralValue::Exact(BigDecimal::from(v)), RelDataType::create_sql_type(s, name))
        };
        let node = match lt {
            LiteralType::Boolean(b) => self.lit(LiteralValue::Boolean(*b), self.not_null(SqlTypeName::Boolean)),
            LiteralType::I8(v) => exact(i64::from(*v as i8), SqlTypeName::Tinyint),
            LiteralType::I16(v) => exact(i64::from(*v as i16), SqlTypeName::Smallint),
            LiteralType::I32(v) => exact(i64::from(*v), SqlTypeName::Integer),
            LiteralType::I64(v) => exact(*v, SqlTypeName::Bigint),
            LiteralType::Fp32(v) => self.lit(LiteralValue::Approx(f64::from(*v)), self.not_null(SqlTypeName::Real)),
            LiteralType::Fp64(v) => self.lit(LiteralValue::Approx(*v), self.not_null(SqlTypeName::Double)),
            LiteralType::String(v) => self.string_literal(v, RelDataType::create_sql_type(s, SqlTypeName::Varchar), nullable),
            LiteralType::VarChar(v) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Varchar, v.length as i32);
                self.string_literal(&v.value, ty, nullable)
            }
            LiteralType::FixedChar(v) => {
                let len = v.chars().count() as i32;
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Char, len.max(1));
                self.lit(LiteralValue::Char(v.clone()), ty)
            }
            LiteralType::Binary(v) => {
                let ty = RelDataType::create_sql_type(s, SqlTypeName::Varbinary);
                self.binary_literal(v, ty, nullable)
            }
            LiteralType::FixedBinary(v) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Binary, v.len() as i32);
                self.lit(LiteralValue::Binary(BitString::create_from_bytes(v)), ty)
            }
            LiteralType::Date(d) => self.lit(
                LiteralValue::Date(DateString::from_days_since_epoch(*d)),
                self.not_null(SqlTypeName::Date),
            ),
            LiteralType::Time(micros) => {
                let seconds = micros / 1_000_000;
                let nanos = ((micros - seconds * 1_000_000) * 1_000) as u32;
                let ts = TimeString::from_millis_of_day((seconds * 1000) as i32).with_nanos(nanos);
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Time, 6);
                self.lit(LiteralValue::Time(ts), ty)
            }
            LiteralType::Timestamp(micros) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Timestamp, 6);
                self.lit(LiteralValue::Timestamp(timestamp_string(*micros, 6)?), ty)
            }
            LiteralType::TimestampTz(micros) => {
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::TimestampWithLocalTimeZone, 6);
                self.lit(LiteralValue::Timestamp(timestamp_string(*micros, 6)?), ty)
            }
            LiteralType::PrecisionTimestamp(p) => {
                let max = s.max_precision(SqlTypeName::Timestamp);
                if p.precision > max {
                    return Err(SubstraitError::unsupported(format!(
                        "unsupported precision_timestamp precision {}, max precision in Calcite type system is set to {max}",
                        p.precision
                    )));
                }
                let value = timestamp_string(p.value, p.precision)?;
                let ty = RelDataType::create_sql_type_with_precision(s, SqlTypeName::Timestamp, p.precision);
                self.lit(LiteralValue::Timestamp(value), ty)
            }
            LiteralType::PrecisionTimestampTz(p) => {
                let max = s.max_precision(SqlTypeName::TimestampWithLocalTimeZone);
                if p.precision > max {
                    return Err(SubstraitError::unsupported(format!(
                        "unsupported precision_timestamp_tz precision {}, max precision in Calcite type system is set to {max}",
                        p.precision
                    )));
                }
                let value = timestamp_string(p.value, p.precision)?;
                let ty = RelDataType::create_sql_type_with_precision(
                    s,
                    SqlTypeName::TimestampWithLocalTimeZone,
                    p.precision,
                );
                self.lit(LiteralValue::Timestamp(value), ty)
            }
            LiteralType::Decimal(d) => {
                if d.value.len() != 16 {
                    return Err(SubstraitError::illegal_argument(
                        "Decimal value must be 16 bytes",
                    ));
                }
                let value = big_decimal_from_bytes(&d.value, i64::from(d.scale));
                let ty = RelDataType::create_sql_type_with_scale(s, SqlTypeName::Decimal, d.precision, d.scale);
                self.lit(LiteralValue::Exact(value), ty)
            }
            LiteralType::IntervalYearToMonth(iy) => {
                let months = i64::from(iy.years) * 12 + i64::from(iy.months);
                self.lit(
                    LiteralValue::Interval(BigDecimal::from(months)),
                    self.not_null(SqlTypeName::IntervalYearMonth),
                )
            }
            LiteralType::IntervalDayToSecond(id) => {
                use crate::proto::expression::literal::interval_day_to_second::PrecisionMode;
                let (precision, subseconds) = match id.precision_mode {
                    Some(PrecisionMode::Precision(p)) => (p, id.subseconds),
                    Some(PrecisionMode::Microseconds(m)) => (6, i64::from(m)),
                    None => (6, 0),
                };
                let millis = if precision > 3 {
                    subseconds / 10i64.pow((precision - 3) as u32)
                } else {
                    subseconds * 10i64.pow((3 - precision) as u32)
                };
                let total = i64::from(id.days) * 86_400_000 + i64::from(id.seconds) * 1000 + millis;
                self.lit(
                    LiteralValue::Interval(BigDecimal::from(total)),
                    self.not_null(SqlTypeName::IntervalDaySecond),
                )
            }
            LiteralType::Null(t) => {
                let ty = to_calcite(s, t)?;
                self.rex_builder.make_null_literal(ty)
            }
            LiteralType::List(list) => {
                let mut args = Vec::with_capacity(list.values.len());
                for v in &list.values {
                    args.push(self.literal(v)?);
                }
                let component = args
                    .first()
                    .map(|a| a.get_type().clone())
                    .unwrap_or_else(|| self.not_null(SqlTypeName::Null));
                let ty = RelDataType::array(least_restrictive_or(&args, component), false);
                self.rex_builder
                    .make_call_typed(ty, &operators::ARRAY_VALUE_CONSTRUCTOR, args)
            }
            LiteralType::EmptyList(l) => {
                let element = l
                    .r#type
                    .as_deref()
                    .ok_or_else(|| SubstraitError::unsupported("Unsupported type: KIND_NOT_SET"))?;
                let ty = RelDataType::array(to_calcite(s, element)?, super::type_converter::is_nullable(l.nullability));
                self.rex_builder
                    .make_call_typed(ty, &operators::ARRAY_VALUE_CONSTRUCTOR, Vec::new())
            }
            other => {
                return Err(SubstraitError::unsupported(format!(
                    "Expression {} of type io.substrait.expression.Expression not handled by visitor type io.substrait.isthmus.expression.ExpressionRexConverter.",
                    crate::substrait_literal_converter::literal_type_case_name(other)
                )))
            }
        };
        Ok(node)
    }

    fn string_literal(&self, value: &str, ty: RelDataType, nullable: bool) -> RexNode {
        let len = value.chars().count() as i32;
        let precision = ty.get_precision();
        let literal = if precision < 0 || precision >= len {
            self.lit(LiteralValue::Char(value.to_string()), ty.clone())
        } else {
            let char_ty = RelDataType::create_sql_type_with_precision(self.system, SqlTypeName::Char, len.max(1));
            let inner = self.lit(LiteralValue::Char(value.to_string()), char_ty);
            self.rex_builder.make_cast(ty.clone(), inner, false)
        };
        if nullable {
            return self.rex_builder.make_cast(ty.with_nullability(true), literal, false);
        }
        literal
    }

    fn binary_literal(&self, value: &[u8], ty: RelDataType, nullable: bool) -> RexNode {
        let literal = self.lit(LiteralValue::Binary(BitString::create_from_bytes(value)), ty.clone());
        if nullable {
            return self.rex_builder.make_cast(ty.with_nullability(true), literal, false);
        }
        literal
    }
}

fn same_family(a: SqlTypeFamily, b: SqlTypeFamily) -> bool {
    a == b
}

fn least_restrictive_or(args: &[RexNode], fallback: RelDataType) -> RelDataType {
    if args.is_empty() {
        return fallback;
    }
    let types: Vec<RelDataType> = args.iter().map(|a| a.get_type().clone()).collect();
    least_restrictive(&types)
}

pub(crate) fn least_restrictive(types: &[RelDataType]) -> RelDataType {
    let nullable = types.iter().any(RelDataType::is_nullable);
    let non_null: Vec<&RelDataType> = types
        .iter()
        .filter(|t| t.get_sql_type_name() != SqlTypeName::Null)
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
    let an = a.get_sql_type_name();
    let bn = b.get_sql_type_name();
    if an.is_character() && bn.is_character() {
        let varchar = an == SqlTypeName::Varchar || bn == SqlTypeName::Varchar;
        let pa = a.get_precision();
        let pb = b.get_precision();
        let precision = if pa < 0 || pb < 0 { -1 } else { pa.max(pb) };
        let name = if varchar {
            SqlTypeName::Varchar
        } else {
            SqlTypeName::Char
        };
        return RelDataType::basic(name, precision, i32::MIN, false);
    }
    if an == bn {
        if a.get_precision() >= b.get_precision() {
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
        values.push(args[i].get_type().clone());
        i += 2;
    }
    if let Some(last) = args.last() {
        values.push(last.get_type().clone());
    }
    least_restrictive(&values)
}
