use crate::calcite::operators::{self, SqlKind};
use crate::calcite::rel_data_type::RelDataType;
use crate::calcite::sql_node::{
    LiteralObject, SqlCall, SqlCase, SqlLiteral, SqlLiteralValue, SqlNode, SqlSelect, SqlWindow,
};
use crate::calcite::sql_type_name::SqlTypeName;
use crate::error::{Result, SubstraitError};
use crate::substrait_type_and_value::{SubstraitTypeAndValue, SubstraitValue};

pub fn parameterize(
    mut node: SqlNode,
    schema: &RelDataType,
) -> Result<(SqlNode, Vec<SubstraitTypeAndValue>)> {
    let mut rewriter = Rewriter {
        schema,
        params: Vec::new(),
    };
    rewriter.rewrite(&mut node, None, false)?;
    Ok((node, rewriter.params))
}

struct Rewriter<'a> {
    schema: &'a RelDataType,
    params: Vec<SubstraitTypeAndValue>,
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

impl Rewriter<'_> {
    fn rewrite(
        &mut self,
        node: &mut SqlNode,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        match node {
            SqlNode::Select(select) => {
                self.rewrite_select(select, column, in_where)?;
                Ok(true)
            }
            SqlNode::Call(call) => {
                if let Some(name) = self.negated_boolean_column(call) {
                    *node = call.operands.swap_remove(0);
                    self.bool_equals(node, &name, false)?;
                    return Ok(true);
                }
                self.rewrite_call(call, column, in_where)
            }
            SqlNode::Identifier(_) => {
                let Some(name) = simple_name(node)
                    .filter(|name| in_where && self.is_boolean_column(name))
                    .map(str::to_string)
                else {
                    return Ok(false);
                };
                self.bool_equals(node, &name, true)?;
                Ok(true)
            }
            SqlNode::Literal(literal) => {
                let Some(column) = column else {
                    tracing::debug!(
                        "Standalone literal {} without column context, skipping",
                        literal.to_value()
                    );
                    return Ok(false);
                };
                let index = self.add_param(column, literal)?;
                *node = SqlNode::DynamicParam(index);
                Ok(true)
            }
            SqlNode::NodeList(list) => self.rewrite_all(list, column, in_where),
            SqlNode::Case(case) => self.rewrite_case(case, column, in_where),
            SqlNode::Window(window) => self.rewrite_window(window, column, in_where),
            SqlNode::DynamicParam(_) | SqlNode::DataType(_) | SqlNode::Text(_) => Ok(false),
        }
    }

    fn rewrite_all(
        &mut self,
        list: &mut [SqlNode],
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = false;
        for node in list {
            changed |= self.rewrite(node, column, in_where)?;
        }
        Ok(changed)
    }

    fn rewrite_opt(
        &mut self,
        node: Option<&mut SqlNode>,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        match node {
            Some(node) => self.rewrite(node, column, in_where),
            None => Ok(false),
        }
    }

    fn rewrite_case(
        &mut self,
        case: &mut SqlCase,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = self.rewrite_opt(case.value.as_deref_mut(), column, in_where)?;
        changed |= self.rewrite_all(&mut case.when_list, column, in_where)?;
        changed |= self.rewrite_all(&mut case.then_list, column, in_where)?;
        changed |= self.rewrite_opt(case.else_expr.as_deref_mut(), column, in_where)?;
        Ok(changed)
    }

    fn rewrite_window(
        &mut self,
        window: &mut SqlWindow,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = self.rewrite_all(&mut window.partition_list, column, in_where)?;
        changed |= self.rewrite_all(&mut window.order_list, column, in_where)?;
        changed |= self.rewrite_opt(window.lower_bound.as_deref_mut(), column, in_where)?;
        changed |= self.rewrite_opt(window.upper_bound.as_deref_mut(), column, in_where)?;
        Ok(changed)
    }

    fn rewrite_select(
        &mut self,
        select: &mut SqlSelect,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<()> {
        self.rewrite_opt(select.where_clause.as_mut(), column, true)?;
        self.rewrite_all(&mut select.keyword_list, column, in_where)?;
        self.rewrite_all(&mut select.select_list, column, in_where)?;
        self.rewrite_opt(select.from.as_mut(), column, in_where)?;
        if let Some(group_by) = &mut select.group_by {
            self.rewrite_all(group_by, column, in_where)?;
        }
        self.rewrite_opt(select.having.as_mut(), column, in_where)?;
        self.rewrite_all(&mut select.window_decls, column, in_where)?;
        self.rewrite_opt(select.qualify.as_mut(), column, in_where)?;
        if let Some(order_by) = &mut select.order_by {
            self.rewrite_all(order_by, column, in_where)?;
        }
        self.rewrite_opt(select.offset.as_mut(), column, in_where)?;
        self.rewrite_opt(select.fetch.as_mut(), column, in_where)?;
        Ok(())
    }

    fn rewrite_call(
        &mut self,
        call: &mut SqlCall,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let kind = call.op.kind;
        let rewritten = match kind {
            SqlKind::And | SqlKind::Or => return self.rewrite_logical(call, column, in_where),
            _ if is_binary_comparison(kind) => self.rewrite_comparison(call, in_where)?,
            SqlKind::In => self.rewrite_in(call, in_where)?,
            SqlKind::Between => self.rewrite_between(call, in_where)?,
            SqlKind::Like => self.rewrite_like(call, in_where)?,
            _ => false,
        };
        if rewritten {
            return Ok(true);
        }
        self.rewrite_all(&mut call.operands, column, in_where)
    }

    fn rewrite_logical(
        &mut self,
        call: &mut SqlCall,
        column: Option<&str>,
        in_where: bool,
    ) -> Result<bool> {
        let mut changed = false;
        for operand in &mut call.operands {
            let boolean_column = simple_name(operand)
                .filter(|name| self.is_boolean_column(name))
                .map(str::to_string);
            match boolean_column {
                Some(name) => {
                    self.bool_equals(operand, &name, true)?;
                    changed = true;
                }
                None => changed |= self.rewrite(operand, column, in_where)?,
            }
        }
        Ok(changed)
    }

    fn rewrite_comparison(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let (Some(left), Some(right)) = (call.operands.first(), call.operands.get(1)) else {
            return Ok(false);
        };
        let target = match simple_name(left) {
            Some(name) => Some((name.to_string(), 1)),
            None => simple_name(right).map(|name| (name.to_string(), 0)),
        };
        match target {
            Some((name, value_index)) => {
                self.rewrite(&mut call.operands[value_index], Some(&name), in_where)
            }
            None => Ok(false),
        }
    }

    fn rewrite_in(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        match call.operands.get_mut(1) {
            Some(SqlNode::NodeList(values)) => self.rewrite_all(values, Some(&name), in_where),
            _ => Ok(false),
        }
    }

    fn rewrite_between(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        let [_, lower, upper, ..] = call.operands.as_mut_slice() else {
            return Ok(false);
        };
        let lower_changed = self.rewrite(lower, Some(&name), in_where)?;
        let upper_changed = self.rewrite(upper, Some(&name), in_where)?;
        Ok(lower_changed || upper_changed)
    }

    fn rewrite_like(&mut self, call: &mut SqlCall, in_where: bool) -> Result<bool> {
        let Some(name) = call
            .operands
            .first()
            .and_then(simple_name)
            .map(str::to_string)
        else {
            return Ok(false);
        };
        match call.operands.get_mut(1) {
            Some(pattern) => self.rewrite(pattern, Some(&name), in_where),
            None => Ok(false),
        }
    }

    fn negated_boolean_column(&self, call: &SqlCall) -> Option<String> {
        if call.op.kind != SqlKind::Not {
            return None;
        }
        match call.operands.as_slice() {
            [operand] => simple_name(operand)
                .filter(|name| self.is_boolean_column(name))
                .map(str::to_string),
            _ => None,
        }
    }

    fn bool_equals(&mut self, slot: &mut SqlNode, name: &str, value: bool) -> Result<()> {
        let index = self.add_param(name, &SqlLiteral::boolean(value))?;
        let identifier = std::mem::replace(slot, SqlNode::DynamicParam(index));
        *slot = SqlNode::call(
            &operators::EQUALS,
            vec![identifier, SqlNode::DynamicParam(index)],
        );
        Ok(())
    }

    fn add_param(&mut self, column_name: &str, literal: &SqlLiteral) -> Result<usize> {
        let field = self
            .schema
            .get_field(column_name, true, true)
            .ok_or_else(|| {
                SubstraitError::IllegalArgument(format!(
                    "field {column_name} not found in schema with fields: [{}]",
                    self.schema.field_names().join(", ")
                ))
            })?;
        let type_name: SqlTypeName = field.get_type().sql_type_name();
        self.params.push(SubstraitTypeAndValue::new(
            type_name,
            literal_value(literal),
            column_name,
        )?);
        Ok(self.params.len() - 1)
    }

    fn is_boolean_column(&self, column_name: &str) -> bool {
        self.schema
            .get_field(column_name, true, true)
            .is_some_and(|f| f.get_type().sql_type_name() == SqlTypeName::Boolean)
    }
}
