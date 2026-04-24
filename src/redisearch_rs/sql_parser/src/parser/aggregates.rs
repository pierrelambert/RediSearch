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
    BinaryOperator, Expr, FunctionArg, FunctionArgExpr, FunctionArguments, GroupByExpr,
};

use crate::ast::{AggregateExpr, AggregateFunction, Condition, GroupBy};
use crate::error::SqlError;

use super::expressions::{extract_identifier, extract_value};

/// Parse an aggregate function from a SQL function expression.
pub(super) fn parse_aggregate_function(
    func: &sqlparser::ast::Function,
    alias: Option<String>,
) -> Result<Option<AggregateExpr>, SqlError> {
    let name = func.name.to_string().to_uppercase();
    let args = extract_function_args(func)?;

    // Match simple single-argument aggregates
    let result = match name.as_str() {
        "COUNT" => {
            let field = if args.is_empty() || args.first().map(|a| a.as_str()) == Some("*") {
                None
            } else {
                Some(args.into_iter().next().unwrap())
            };
            Some((AggregateFunction::Count, field))
        }
        "SUM" => Some((AggregateFunction::Sum, args.into_iter().next())),
        "AVG" => Some((AggregateFunction::Avg, args.into_iter().next())),
        "MIN" => Some((AggregateFunction::Min, args.into_iter().next())),
        "MAX" => Some((AggregateFunction::Max, args.into_iter().next())),
        "COUNT_DISTINCT" => Some((AggregateFunction::CountDistinct, args.into_iter().next())),
        "COUNT_DISTINCTISH" => Some((AggregateFunction::CountDistinctish, args.into_iter().next())),
        "STDDEV" => Some((AggregateFunction::Stddev, args.into_iter().next())),
        "TOLIST" => Some((AggregateFunction::Tolist, args.into_iter().next())),
        "HLL" => Some((AggregateFunction::Hll, args.into_iter().next())),
        "HLL_SUM" => Some((AggregateFunction::HllSum, args.into_iter().next())),
        "QUANTILE" => {
            // QUANTILE(field, percentile) - requires 2 arguments
            if args.len() != 2 {
                return Err(SqlError::syntax(
                    "QUANTILE requires exactly 2 arguments: QUANTILE(field, percentile)",
                ));
            }
            let field = args[0].clone();
            let percentile: f64 = args[1].parse().map_err(|_| {
                SqlError::syntax(format!(
                    "QUANTILE percentile must be a number between 0.0 and 1.0, got: '{}'",
                    args[1]
                ))
            })?;
            if !(0.0..=1.0).contains(&percentile) {
                return Err(SqlError::syntax(format!(
                    "QUANTILE percentile must be between 0.0 and 1.0, got: {}",
                    percentile
                )));
            }
            Some((AggregateFunction::Quantile { percentile }, Some(field)))
        }
        "RANDOM_SAMPLE" => {
            // RANDOM_SAMPLE(field, size) - requires 2 arguments
            if args.len() != 2 {
                return Err(SqlError::syntax(
                    "RANDOM_SAMPLE requires exactly 2 arguments: RANDOM_SAMPLE(field, size)",
                ));
            }
            let field = args[0].clone();
            let size: u32 = args[1].parse().map_err(|_| {
                SqlError::syntax(format!(
                    "RANDOM_SAMPLE size must be a positive integer, got: '{}'",
                    args[1]
                ))
            })?;
            if size == 0 || size > 1000 {
                return Err(SqlError::syntax(format!(
                    "RANDOM_SAMPLE size must be between 1 and 1000, got: {}",
                    size
                )));
            }
            Some((AggregateFunction::RandomSample { size }, Some(field)))
        }
        "FIRST_VALUE" => {
            // FIRST_VALUE(field, sort_field) or FIRST_VALUE(field, sort_field, 'ASC'/'DESC')
            // Simplified syntax since SQL standard window function syntax is complex
            if args.len() < 2 || args.len() > 3 {
                return Err(SqlError::syntax(
                    "FIRST_VALUE requires 2-3 arguments: FIRST_VALUE(field, sort_field [, 'ASC'|'DESC'])",
                ));
            }
            let field = args[0].clone();
            let sort_field = args[1].clone();
            let ascending = if args.len() == 3 {
                match args[2].to_uppercase().as_str() {
                    "ASC" => true,
                    "DESC" => false,
                    _ => {
                        return Err(SqlError::syntax(
                            "FIRST_VALUE third argument must be 'ASC' or 'DESC'",
                        ));
                    }
                }
            } else {
                false // Default to DESC as per RediSearch convention
            };
            Some((
                AggregateFunction::FirstValue {
                    sort_field,
                    ascending,
                },
                Some(field),
            ))
        }
        _ => None,
    };

    match result {
        Some((function, field)) => Ok(Some(AggregateExpr {
            function,
            field,
            alias,
        })),
        None => Ok(None),
    }
}

/// Extract function arguments as strings.
fn extract_function_args(func: &sqlparser::ast::Function) -> Result<Vec<String>, SqlError> {
    match &func.args {
        FunctionArguments::List(arg_list) => {
            let mut args = Vec::new();
            for arg in &arg_list.args {
                match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {
                        args.push("*".to_string());
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                        // Handle numeric literals, identifiers, and string literals
                        match expr {
                            Expr::Identifier(ident) => args.push(ident.value.clone()),
                            Expr::Value(sqlparser::ast::Value::Number(n, _)) => {
                                args.push(n.clone())
                            }
                            Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
                            | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => {
                                args.push(s.clone())
                            }
                            Expr::CompoundIdentifier(parts) => {
                                if let Some(last) = parts.last() {
                                    args.push(last.value.clone());
                                }
                            }
                            _ => {
                                return Err(SqlError::unsupported(format!(
                                    "Unsupported function argument expression: {expr:?}"
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(SqlError::unsupported("Unsupported function argument type"));
                    }
                }
            }
            Ok(args)
        }
        FunctionArguments::None => Ok(Vec::new()),
        FunctionArguments::Subquery(_) => Err(SqlError::unsupported(
            "Subqueries in aggregate functions are not supported",
        )),
    }
}

/// Parse GROUP BY clause.
pub(super) fn parse_group_by(group_by: &GroupByExpr) -> Result<Option<GroupBy>, SqlError> {
    match group_by {
        GroupByExpr::All(_) => Err(SqlError::unsupported("GROUP BY ALL is not supported")),
        GroupByExpr::Expressions(exprs, _) => {
            if exprs.is_empty() {
                return Ok(None);
            }
            let fields: Vec<String> = exprs
                .iter()
                .map(extract_identifier)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(GroupBy { fields }))
        }
    }
}

/// Parse HAVING expression (supports aggregate functions and AND/OR).
pub(super) fn parse_having_expression(expr: &Expr) -> Result<Condition, SqlError> {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            // Handle AND/OR logical operators
            match op {
                BinaryOperator::And => {
                    let left_cond = parse_having_expression(left)?;
                    let right_cond = parse_having_expression(right)?;
                    return Ok(Condition::And(Box::new(left_cond), Box::new(right_cond)));
                }
                BinaryOperator::Or => {
                    let left_cond = parse_having_expression(left)?;
                    let right_cond = parse_having_expression(right)?;
                    return Ok(Condition::Or(Box::new(left_cond), Box::new(right_cond)));
                }
                _ => {}
            }

            // In HAVING, the left side is typically an aggregate function or alias
            let field = extract_having_field(left)?;
            let value = extract_value(right)?;

            match op {
                BinaryOperator::Gt => Ok(Condition::GreaterThan { field, value }),
                BinaryOperator::GtEq => Ok(Condition::GreaterThanOrEqual { field, value }),
                BinaryOperator::Lt => Ok(Condition::LessThan { field, value }),
                BinaryOperator::LtEq => Ok(Condition::LessThanOrEqual { field, value }),
                BinaryOperator::Eq => Ok(Condition::Equals { field, value }),
                BinaryOperator::NotEq => Ok(Condition::NotEquals { field, value }),
                _ => Err(SqlError::unsupported(format!(
                    "Operator {op:?} not supported in HAVING clause"
                ))),
            }
        }
        Expr::Nested(inner) => {
            // Handle parenthesized expressions like (COUNT(*) > 5)
            parse_having_expression(inner)
        }
        _ => Err(SqlError::unsupported(
            "Only simple comparisons are supported in HAVING clause",
        )),
    }
}

/// Extract field name from HAVING expression (handles aggregates).
fn extract_having_field(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        Expr::Function(func) => {
            // For aggregate functions like COUNT(*), generate a field name
            let func_name = func.name.to_string().to_uppercase();
            match &func.args {
                FunctionArguments::List(arg_list) => {
                    if arg_list.args.is_empty()
                        || matches!(
                            arg_list.args.first(),
                            Some(FunctionArg::Unnamed(FunctionArgExpr::Wildcard))
                        )
                    {
                        Ok(func_name.to_lowercase())
                    } else if arg_list.args.len() == 1 {
                        if let Some(FunctionArg::Unnamed(FunctionArgExpr::Expr(e))) =
                            arg_list.args.first()
                        {
                            let field = extract_identifier(e)?;
                            Ok(format!("{}_{}", func_name.to_lowercase(), field))
                        } else {
                            Ok(func_name.to_lowercase())
                        }
                    } else {
                        Ok(func_name.to_lowercase())
                    }
                }
                _ => Ok(func_name.to_lowercase()),
            }
        }
        _ => Err(SqlError::unsupported(format!(
            "Unsupported expression in HAVING: {expr:?}"
        ))),
    }
}

#[cfg(test)]
#[path = "aggregates_tests.rs"]
mod tests;
