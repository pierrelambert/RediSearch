/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use sqlparser::ast::{
    BinaryOperator, Expr, Offset, OffsetRows, OrderByExpr, SelectItem, SetExpr, Statement,
    TableFactor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::ast::{Condition, Limit, OrderBy, SelectQuery, SortDirection, Value};
use crate::error::SqlError;

/// Parses a SQL query string into our internal AST representation.
pub fn parse(sql: &str) -> Result<SelectQuery, SqlError> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, sql)?;

    if statements.is_empty() {
        return Err(SqlError::syntax("Empty query"));
    }

    if statements.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple statements are not supported",
        ));
    }

    let statement = statements.into_iter().next().unwrap();
    parse_statement(statement)
}

fn parse_statement(statement: Statement) -> Result<SelectQuery, SqlError> {
    match statement {
        Statement::Query(query) => {
            let select = match *query.body {
                SetExpr::Select(select) => select,
                _ => {
                    return Err(SqlError::unsupported(
                        "Only SELECT queries are supported in Phase 1",
                    ))
                }
            };

            // Parse fields from SELECT clause
            let fields = parse_select_items(&select.projection)?;

            // Parse FROM clause
            let index_name = parse_from_clause(&select.from)?;

            // Parse WHERE clause
            let conditions = if let Some(selection) = select.selection {
                parse_where_clause(&selection)?
            } else {
                Vec::new()
            };

            // Parse ORDER BY clause
            let order_by = if let Some(ref ob) = query.order_by {
                parse_order_by(&ob.exprs)?
            } else {
                None
            };

            // Parse LIMIT/OFFSET clause
            let limit = parse_limit(query.limit.as_ref(), query.offset.as_ref())?;

            Ok(SelectQuery {
                fields,
                index_name,
                conditions,
                order_by,
                limit,
            })
        }
        _ => Err(SqlError::unsupported(
            "Only SELECT statements are supported in Phase 1",
        )),
    }
}

fn parse_select_items(items: &[SelectItem]) -> Result<Vec<String>, SqlError> {
    let mut fields = Vec::new();

    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                // SELECT * - return empty vec to indicate all fields
                return Ok(Vec::new());
            }
            SelectItem::UnnamedExpr(expr) => {
                let field_name = extract_identifier(expr)?;
                fields.push(field_name);
            }
            SelectItem::ExprWithAlias { expr, .. } => {
                let field_name = extract_identifier(expr)?;
                fields.push(field_name);
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "Unsupported SELECT item: {item:?}"
                )))
            }
        }
    }

    Ok(fields)
}

fn parse_from_clause(
    from: &[sqlparser::ast::TableWithJoins],
) -> Result<String, SqlError> {
    if from.is_empty() {
        return Err(SqlError::syntax("FROM clause is required"));
    }

    if from.len() > 1 {
        return Err(SqlError::unsupported("JOINs are not supported in Phase 1"));
    }

    let table = &from[0];
    if !table.joins.is_empty() {
        return Err(SqlError::unsupported("JOINs are not supported in Phase 1"));
    }

    match &table.relation {
        TableFactor::Table { name, .. } => {
            Ok(name.to_string())
        }
        _ => Err(SqlError::unsupported("Only simple table references are supported")),
    }
}

fn parse_where_clause(expr: &Expr) -> Result<Vec<Condition>, SqlError> {
    let mut conditions = Vec::new();
    parse_expression(expr, &mut conditions)?;
    Ok(conditions)
}

fn parse_expression(expr: &Expr, conditions: &mut Vec<Condition>) -> Result<(), SqlError> {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            parse_binary_op(left, op, right, conditions)
        }
        Expr::Between { expr, low, high, negated: false } => {
            let field = extract_identifier(expr)?;
            let low_val = extract_value(low)?;
            let high_val = extract_value(high)?;
            conditions.push(Condition::Between {
                field,
                low: low_val,
                high: high_val,
            });
            Ok(())
        }
        Expr::Between { negated: true, .. } => {
            Err(SqlError::unsupported("NOT BETWEEN is not supported in Phase 1"))
        }
        Expr::Nested(inner) => parse_expression(inner, conditions),
        _ => Err(SqlError::unsupported(format!(
            "Unsupported expression type: {expr:?}"
        ))),
    }
}

fn parse_binary_op(
    left: &Expr,
    op: &BinaryOperator,
    right: &Expr,
    conditions: &mut Vec<Condition>,
) -> Result<(), SqlError> {
    match op {
        BinaryOperator::Eq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::Equals { field, value });
            Ok(())
        }
        BinaryOperator::Gt => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::GreaterThan { field, value });
            Ok(())
        }
        BinaryOperator::GtEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::GreaterThanOrEqual { field, value });
            Ok(())
        }
        BinaryOperator::Lt => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::LessThan { field, value });
            Ok(())
        }
        BinaryOperator::LtEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::LessThanOrEqual { field, value });
            Ok(())
        }
        BinaryOperator::And => {
            // For AND, recursively parse both sides
            parse_expression(left, conditions)?;
            parse_expression(right, conditions)?;
            Ok(())
        }
        _ => Err(SqlError::unsupported(format!(
            "Operator {op:?} is not supported in Phase 1"
        ))),
    }
}

fn extract_identifier(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        Expr::CompoundIdentifier(parts) => {
            // For compound identifiers like table.column, just use the last part
            Ok(parts.last().map(|i| i.value.clone()).unwrap_or_default())
        }
        _ => Err(SqlError::translation(format!(
            "Expected identifier, got: {expr:?}"
        ))),
    }
}

fn extract_value(expr: &Expr) -> Result<Value, SqlError> {
    match expr {
        Expr::Value(val) => match val {
            sqlparser::ast::Value::Number(n, _) => {
                let num: f64 = n.parse().map_err(|_| {
                    SqlError::translation(format!("Invalid number: {n}"))
                })?;
                Ok(Value::Number(num))
            }
            sqlparser::ast::Value::SingleQuotedString(s)
            | sqlparser::ast::Value::DoubleQuotedString(s) => {
                Ok(Value::String(s.clone()))
            }
            _ => Err(SqlError::unsupported(format!(
                "Unsupported value type: {val:?}"
            ))),
        },
        Expr::UnaryOp { op: sqlparser::ast::UnaryOperator::Minus, expr } => {
            // Handle negative numbers
            if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = expr.as_ref() {
                let num: f64 = n.parse().map_err(|_| {
                    SqlError::translation(format!("Invalid number: {n}"))
                })?;
                return Ok(Value::Number(-num));
            }
            Err(SqlError::translation(format!(
                "Expected numeric value, got: {expr:?}"
            )))
        }
        _ => Err(SqlError::translation(format!(
            "Expected value, got: {expr:?}"
        ))),
    }
}

fn parse_order_by(order_by: &[OrderByExpr]) -> Result<Option<OrderBy>, SqlError> {
    if order_by.is_empty() {
        return Ok(None);
    }

    if order_by.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple ORDER BY columns are not supported in Phase 1",
        ));
    }

    let order_expr = &order_by[0];
    let field = extract_identifier(&order_expr.expr)?;

    let direction = if order_expr.asc.unwrap_or(true) {
        SortDirection::Asc
    } else {
        SortDirection::Desc
    };

    Ok(Some(OrderBy { field, direction }))
}

fn parse_limit(
    limit: Option<&Expr>,
    offset: Option<&Offset>,
) -> Result<Option<Limit>, SqlError> {
    let count = match limit {
        Some(expr) => extract_limit_value(expr)?,
        None => return Ok(None),
    };

    let offset_value = match offset {
        Some(off) => {
            match &off.rows {
                OffsetRows::None | OffsetRows::Row | OffsetRows::Rows => {}
            }
            extract_limit_value(&off.value)?
        }
        None => 0,
    };

    Ok(Some(Limit {
        count,
        offset: offset_value,
    }))
}

fn extract_limit_value(expr: &Expr) -> Result<u64, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::Number(n, _)) => {
            n.parse().map_err(|_| {
                SqlError::translation(format!("Invalid LIMIT value: {n}"))
            })
        }
        _ => Err(SqlError::translation("LIMIT value must be a number")),
    }
}

