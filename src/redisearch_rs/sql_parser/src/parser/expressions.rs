/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use sqlparser::ast::{BinaryOperator, Expr, UnaryOperator};

use crate::ast::{Condition, Value};
use crate::error::SqlError;

pub(super) fn parse_where_clause(expr: &Expr) -> Result<Vec<Condition>, SqlError> {
    let mut conditions = Vec::new();
    parse_expression(expr, &mut conditions)?;
    Ok(conditions)
}

fn unsupported_complex_boolean_expression(examples: &str) -> SqlError {
    SqlError::unsupported(format!(
        "Complex boolean expressions like {examples} are not yet supported; supported WHERE boolean forms are simple `a AND b`, simple `a OR b`, and `NOT <single predicate>`"
    ))
}

fn parse_expression(expr: &Expr, conditions: &mut Vec<Condition>) -> Result<(), SqlError> {
    match expr {
        Expr::BinaryOp { left, op, right } => parse_binary_op(left, op, right, conditions),
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
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
        Expr::Between { negated: true, .. } => Err(SqlError::unsupported(
            "NOT BETWEEN is not supported in Phase 1",
        )),
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let field = extract_identifier(expr)?;
            let values: Vec<Value> = list
                .iter()
                .map(extract_value)
                .collect::<Result<Vec<_>, _>>()?;

            if values.is_empty() {
                return Err(SqlError::syntax("IN clause requires at least one value"));
            }

            conditions.push(Condition::In {
                field,
                values,
                negated: *negated,
            });
            Ok(())
        }
        Expr::Nested(inner) => parse_expression(inner, conditions),
        Expr::Like {
            expr,
            pattern,
            negated,
            ..
        } => {
            let field = extract_identifier(expr)?;
            let pattern_str = extract_string_value(pattern)?;
            conditions.push(Condition::Like {
                field,
                pattern: pattern_str,
                negated: *negated,
            });
            Ok(())
        }
        Expr::IsNull(expr) => {
            let field = extract_identifier(expr)?;
            conditions.push(Condition::IsNull {
                field,
                negated: false,
            });
            Ok(())
        }
        Expr::IsNotNull(expr) => {
            let field = extract_identifier(expr)?;
            conditions.push(Condition::IsNull {
                field,
                negated: true,
            });
            Ok(())
        }
        Expr::UnaryOp {
            op: UnaryOperator::Not,
            expr: inner,
        } => {
            // Handle NOT (condition)
            let mut inner_conditions = Vec::new();
            parse_expression(inner, &mut inner_conditions)?;
            if inner_conditions.len() == 1 {
                let inner_cond = inner_conditions.pop().unwrap();
                conditions.push(Condition::Not(Box::new(inner_cond)));
                Ok(())
            } else {
                Err(unsupported_complex_boolean_expression("`NOT (a AND b)`"))
            }
        }
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
        BinaryOperator::NotEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::NotEquals { field, value });
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
        BinaryOperator::Or => {
            // For OR, we need to create a combined OR condition
            let mut left_conditions = Vec::new();
            let mut right_conditions = Vec::new();
            parse_expression(left, &mut left_conditions)?;
            parse_expression(right, &mut right_conditions)?;

            // Each side should have exactly one condition for simple OR
            if left_conditions.len() == 1 && right_conditions.len() == 1 {
                let left_cond = left_conditions.pop().unwrap();
                let right_cond = right_conditions.pop().unwrap();
                conditions.push(Condition::Or(Box::new(left_cond), Box::new(right_cond)));
            } else {
                return Err(unsupported_complex_boolean_expression(
                    "`(a AND b) OR c` or `a OR (b AND c)`",
                ));
            }
            Ok(())
        }
        _ => Err(SqlError::unsupported(format!(
            "Operator {op:?} is not supported"
        ))),
    }
}

pub(super) fn extract_identifier(expr: &Expr) -> Result<String, SqlError> {
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

pub(super) fn extract_value(expr: &Expr) -> Result<Value, SqlError> {
    match expr {
        Expr::Value(val) => match val {
            sqlparser::ast::Value::Number(n, _) => {
                let num: f64 = n
                    .parse()
                    .map_err(|_| SqlError::translation(format!("Invalid number: {n}")))?;
                Ok(Value::Number(num))
            }
            sqlparser::ast::Value::SingleQuotedString(s)
            | sqlparser::ast::Value::DoubleQuotedString(s) => Ok(Value::String(s.clone())),
            _ => Err(SqlError::unsupported(format!(
                "Unsupported value type: {val:?}"
            ))),
        },
        Expr::UnaryOp {
            op: UnaryOperator::Minus,
            expr,
        } => {
            // Handle negative numbers
            if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = expr.as_ref() {
                let num: f64 = n
                    .parse()
                    .map_err(|_| SqlError::translation(format!("Invalid number: {n}")))?;
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

pub(super) fn extract_string_value(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
        | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => Ok(s.clone()),
        _ => Err(SqlError::translation(format!(
            "Expected string value, got: {expr:?}"
        ))),
    }
}

#[cfg(test)]
#[path = "expressions_tests.rs"]
mod tests;
