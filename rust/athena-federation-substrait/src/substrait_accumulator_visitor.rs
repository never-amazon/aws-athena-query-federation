use crate::calcite::operators::{self, SqlKind};
use crate::calcite::rel_data_type::RelDataType;
use crate::calcite::sql_node::{
    LiteralObject, SqlCall, SqlCase, SqlLiteral, SqlLiteralValue, SqlNode, SqlSelect, SqlWindow,
};
use crate::calcite::sql_type_name::SqlTypeName;
use crate::error::{Result, SubstraitError};
use crate::substrait_type_and_value::{SubstraitTypeAndValue, SubstraitValue};

pub struct SubstraitAccumulatorVisitor<'a> {
    accumulator: &'a mut Vec<SubstraitTypeAndValue>,
    schema: &'a RelDataType,
    column_stack: Vec<String>,
    in_where_clause: bool,
}

type Visited = Option<SqlNode>;

fn or_original(visited: Visited, original: &SqlNode) -> SqlNode {
    visited.unwrap_or_else(|| original.clone())
}

fn simple_name(node: &SqlNode) -> Option<&str> {
    match node {
        SqlNode::Identifier(id) if id.is_simple() => id.names.first().map(String::as_str),
        _ => None,
    }
}

fn is_binary_comparison(kind: SqlKind) -> bool {
    matches!(
        kind,
        SqlKind::Equals
            | SqlKind::NotEquals
            | SqlKind::GreaterThan
            | SqlKind::LessThan
            | SqlKind::GreaterThanOrEqual
            | SqlKind::LessThanOrEqual
    )
}

fn rebuild(call: &SqlCall, operands: Vec<SqlNode>) -> SqlNode {
    SqlNode::Call(SqlCall {
        op: call.op,
        operands,
        quantifier: call.quantifier.clone(),
    })
}

fn literal_value(literal: &SqlLiteral) -> SubstraitValue {
    if let SqlLiteralValue::Interval { text, .. } = &literal.value {
        return SubstraitValue::Interval(text.clone());
    }
    match literal.get_value() {
        LiteralObject::Null => SubstraitValue::Null,
        LiteralObject::Boolean(b) => SubstraitValue::Boolean(b),
        LiteralObject::Decimal(d) => SubstraitValue::Decimal(d),
        LiteralObject::String(s) => SubstraitValue::String(s),
        LiteralObject::Date(d) => SubstraitValue::Date(d),
        LiteralObject::Time(t) => SubstraitValue::Time(t),
        LiteralObject::Timestamp(t) => SubstraitValue::Timestamp(t),
        LiteralObject::Bits(b) => SubstraitValue::Bits(b),
        LiteralObject::Symbol(s) => SubstraitValue::Symbol(s),
    }
}

impl<'a> SubstraitAccumulatorVisitor<'a> {
    pub fn new(accumulator: &'a mut Vec<SubstraitTypeAndValue>, schema: &'a RelDataType) -> Self {
        SubstraitAccumulatorVisitor {
            accumulator,
            schema,
            column_stack: Vec::new(),
            in_where_clause: false,
        }
    }

    pub fn visit(&mut self, node: &SqlNode) -> Result<SqlNode> {
        Ok(or_original(self.accept(node)?, node))
    }

    fn accept(&mut self, node: &SqlNode) -> Result<Visited> {
        match node {
            SqlNode::Select(select) => self.handle_select(select).map(Some),
            SqlNode::Call(call) => self.visit_call(call),
            SqlNode::Identifier(_) => self.visit_identifier(node),
            SqlNode::Literal(literal) => self.visit_literal(literal),
            SqlNode::NodeList(list) => Ok(self.visit_list(list)?.map(SqlNode::NodeList)),
            SqlNode::Case(case) => self.visit_case(case),
            SqlNode::Window(window) => self.visit_window(window),
            SqlNode::DynamicParam(_) | SqlNode::DataType(_) | SqlNode::Text(_) => Ok(None),
        }
    }

    fn visit_list(&mut self, list: &[SqlNode]) -> Result<Option<Vec<SqlNode>>> {
        let mut changed = false;
        let mut out = Vec::with_capacity(list.len());
        for node in list {
            match self.accept(node)? {
                Some(n) => {
                    changed = true;
                    out.push(n);
                }
                None => out.push(node.clone()),
            }
        }
        Ok(changed.then_some(out))
    }

    fn visit_opt(&mut self, node: Option<&SqlNode>, changed: &mut bool) -> Result<Option<SqlNode>> {
        match node {
            None => Ok(None),
            Some(n) => match self.accept(n)? {
                Some(v) => {
                    *changed = true;
                    Ok(Some(v))
                }
                None => Ok(Some(n.clone())),
            },
        }
    }

    fn visit_opt_list(
        &mut self,
        list: Option<&[SqlNode]>,
        changed: &mut bool,
    ) -> Result<Option<Vec<SqlNode>>> {
        match list {
            None => Ok(None),
            Some(l) => match self.visit_list(l)? {
                Some(v) => {
                    *changed = true;
                    Ok(Some(v))
                }
                None => Ok(Some(l.to_vec())),
            },
        }
    }

    fn visit_case(&mut self, case: &SqlCase) -> Result<Visited> {
        let mut changed = false;
        let value = self.visit_opt(case.value.as_deref(), &mut changed)?;
        let when_list = self
            .visit_opt_list(Some(&case.when_list), &mut changed)?
            .unwrap_or_default();
        let then_list = self
            .visit_opt_list(Some(&case.then_list), &mut changed)?
            .unwrap_or_default();
        let else_expr = self.visit_opt(case.else_expr.as_deref(), &mut changed)?;
        Ok(changed.then(|| {
            SqlNode::Case(Box::new(SqlCase {
                value: value.map(Box::new),
                when_list,
                then_list,
                else_expr: else_expr.map(Box::new),
            }))
        }))
    }

    fn visit_window(&mut self, window: &SqlWindow) -> Result<Visited> {
        let mut changed = false;
        let partition_list = self
            .visit_opt_list(Some(&window.partition_list), &mut changed)?
            .unwrap_or_default();
        let order_list = self
            .visit_opt_list(Some(&window.order_list), &mut changed)?
            .unwrap_or_default();
        let lower_bound = self.visit_opt(window.lower_bound.as_deref(), &mut changed)?;
        let upper_bound = self.visit_opt(window.upper_bound.as_deref(), &mut changed)?;
        Ok(changed.then(|| {
            SqlNode::Window(Box::new(SqlWindow {
                partition_list,
                order_list,
                is_rows: window.is_rows,
                lower_bound: lower_bound.map(Box::new),
                upper_bound: upper_bound.map(Box::new),
            }))
        }))
    }

    fn super_visit_call(&mut self, call: &SqlCall) -> Result<Visited> {
        Ok(self
            .visit_list(&call.operands)?
            .map(|operands| rebuild(call, operands)))
    }

    fn visit_call(&mut self, call: &SqlCall) -> Result<Visited> {
        let kind = call.op.kind;
        if kind == SqlKind::And || kind == SqlKind::Or {
            return self.handle_logical_operator(call);
        }
        if kind == SqlKind::Not {
            return self.handle_not(call);
        }
        if is_binary_comparison(kind) {
            return self.handle_binary_comparison(call);
        }
        match kind {
            SqlKind::In => self.handle_in(call),
            SqlKind::Between => self.handle_between(call),
            SqlKind::Like => self.handle_like(call),
            _ => self.super_visit_call(call),
        }
    }

    fn visit_identifier(&mut self, node: &SqlNode) -> Result<Visited> {
        let Some(name) = simple_name(node) else {
            return Ok(None);
        };
        if self.in_where_clause && self.is_boolean_column(name) {
            return self.bool_equals(node, name, true).map(Some);
        }
        Ok(None)
    }

    fn visit_literal(&mut self, literal: &SqlLiteral) -> Result<Visited> {
        let Some(column_name) = self.column_stack.last().cloned() else {
            tracing::debug!(
                "Standalone literal {} without column context, skipping",
                literal.to_value()
            );
            return Ok(None);
        };
        self.add_to_accumulator(&column_name, literal)?;
        Ok(Some(SqlNode::DynamicParam(self.accumulator.len() - 1)))
    }

    fn bool_equals(&mut self, identifier: &SqlNode, name: &str, value: bool) -> Result<SqlNode> {
        let literal = SqlLiteral::boolean(value);
        self.add_to_accumulator(name, &literal)?;
        let param = SqlNode::DynamicParam(self.accumulator.len() - 1);
        Ok(SqlNode::call(
            &operators::EQUALS,
            vec![identifier.clone(), param],
        ))
    }

    fn with_column<T>(
        &mut self,
        column: &str,
        f: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.column_stack.push(column.to_string());
        let out = f(self);
        self.column_stack.pop();
        out
    }

    fn handle_binary_comparison(&mut self, call: &SqlCall) -> Result<Visited> {
        let left = call.operands.first();
        let right = call.operands.get(1);
        let target = match (left, right) {
            (Some(l), Some(r)) if simple_name(l).is_some() => {
                simple_name(l).map(|n| (n.to_string(), r, 1))
            }
            (Some(l), Some(r)) => simple_name(r).map(|n| (n.to_string(), l, 0)),
            _ => None,
        };
        if let Some((column_name, value_operand, value_index)) = target {
            let visited = self.with_column(&column_name, |v| v.accept(value_operand))?;
            if let Some(new_value) = visited {
                let mut operands = call.operands.clone();
                operands[value_index] = new_value;
                return Ok(Some(rebuild(call, operands)));
            }
        }
        self.super_visit_call(call)
    }

    fn handle_in(&mut self, call: &SqlCall) -> Result<Visited> {
        let Some(column_name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return self.super_visit_call(call);
        };
        if let Some(SqlNode::NodeList(values)) = call.operands.get(1) {
            let new_list = self.with_column(&column_name, |v| v.visit_list(values))?;
            if let Some(list) = new_list {
                return Ok(Some(rebuild(
                    call,
                    vec![call.operands[0].clone(), SqlNode::NodeList(list)],
                )));
            }
        }
        self.super_visit_call(call)
    }

    fn handle_between(&mut self, call: &SqlCall) -> Result<Visited> {
        let Some(column_name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return self.super_visit_call(call);
        };
        let (Some(lower), Some(upper)) = (call.operands.get(1), call.operands.get(2)) else {
            return self.super_visit_call(call);
        };
        let (new_lower, new_upper) =
            self.with_column(&column_name, |v| Ok((v.accept(lower)?, v.accept(upper)?)))?;
        if new_lower.is_some() || new_upper.is_some() {
            return Ok(Some(rebuild(
                call,
                vec![
                    call.operands[0].clone(),
                    or_original(new_lower, lower),
                    or_original(new_upper, upper),
                ],
            )));
        }
        self.super_visit_call(call)
    }

    fn handle_like(&mut self, call: &SqlCall) -> Result<Visited> {
        let Some(column_name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return self.super_visit_call(call);
        };
        let Some(pattern) = call.operands.get(1) else {
            return self.super_visit_call(call);
        };
        let new_pattern = self.with_column(&column_name, |v| v.accept(pattern))?;
        if let Some(p) = new_pattern {
            let mut operands = vec![call.operands[0].clone(), p];
            if call.operands.len() == 3 {
                operands.push(call.operands[2].clone());
            }
            return Ok(Some(rebuild(call, operands)));
        }
        self.super_visit_call(call)
    }

    fn handle_logical_operator(&mut self, call: &SqlCall) -> Result<Visited> {
        let mut modified = false;
        let mut operands = Vec::with_capacity(call.operands.len());
        for operand in &call.operands {
            if let Some(name) = simple_name(operand) {
                if self.is_boolean_column(name) {
                    let name = name.to_string();
                    operands.push(self.bool_equals(operand, &name, true)?);
                    modified = true;
                    continue;
                }
            }
            match self.accept(operand)? {
                Some(n) => {
                    modified = true;
                    operands.push(n);
                }
                None => operands.push(operand.clone()),
            }
        }
        Ok(modified.then(|| rebuild(call, operands)))
    }

    fn handle_not(&mut self, call: &SqlCall) -> Result<Visited> {
        if call.operands.len() == 1 {
            let operand = &call.operands[0];
            if let Some(name) = simple_name(operand) {
                if self.is_boolean_column(name) {
                    let name = name.to_string();
                    return self.bool_equals(operand, &name, false).map(Some);
                }
            }
        }
        self.super_visit_call(call)
    }

    fn handle_select(&mut self, select: &SqlSelect) -> Result<SqlNode> {
        let new_where = match &select.where_clause {
            Some(w) => {
                let prev = self.in_where_clause;
                self.in_where_clause = true;
                let visited = self.accept(w);
                self.in_where_clause = prev;
                Some(or_original(visited?, w))
            }
            None => None,
        };
        let mut changed = false;
        let keyword_list = self
            .visit_opt_list(Some(&select.keyword_list), &mut changed)?
            .unwrap_or_default();
        let select_list = self
            .visit_opt_list(Some(&select.select_list), &mut changed)?
            .unwrap_or_default();
        let from = self.visit_opt(select.from.as_ref(), &mut changed)?;
        let group_by = self.visit_opt_list(select.group_by.as_deref(), &mut changed)?;
        let having = self.visit_opt(select.having.as_ref(), &mut changed)?;
        let window_decls = self
            .visit_opt_list(Some(&select.window_decls), &mut changed)?
            .unwrap_or_default();
        let qualify = self.visit_opt(select.qualify.as_ref(), &mut changed)?;
        let order_by = self.visit_opt_list(select.order_by.as_deref(), &mut changed)?;
        let offset = self.visit_opt(select.offset.as_ref(), &mut changed)?;
        let fetch = self.visit_opt(select.fetch.as_ref(), &mut changed)?;
        Ok(SqlNode::Select(Box::new(SqlSelect {
            keyword_list,
            select_list,
            from,
            where_clause: new_where,
            group_by,
            having,
            window_decls,
            qualify,
            order_by,
            offset,
            fetch,
        })))
    }

    fn add_to_accumulator(&mut self, column_name: &str, literal: &SqlLiteral) -> Result<()> {
        let field = self
            .schema
            .get_field(column_name, true, true)
            .ok_or_else(|| {
                SubstraitError::illegal_argument(format!(
                    "field {column_name} not found in schema with fields: [{}]",
                    self.schema.field_names().join(", ")
                ))
            })?;
        let type_name: SqlTypeName = field.get_type().sql_type_name();
        let value = literal_value(literal);
        self.accumulator
            .push(SubstraitTypeAndValue::new(type_name, value, column_name)?);
        Ok(())
    }

    fn is_boolean_column(&self, column_name: &str) -> bool {
        self.schema
            .get_field(column_name, true, true)
            .is_some_and(|f| f.get_type().sql_type_name() == SqlTypeName::Boolean)
    }
}
