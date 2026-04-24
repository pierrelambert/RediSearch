/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use sqlparser::ast::{BinaryOperator, Expr, Offset, OffsetRows, OrderByExpr};

use crate::ast::{
    DistanceMetric, HybridSearch, Limit, OrderBy, OrderByColumn, SelectQuery, SortDirection,
    VectorSearch,
};
use crate::error::SqlError;

use super::QueryOptions;
use super::expressions::extract_identifier;

/// Apply OPTION clause to create HybridSearch if weights are specified.
pub(super) fn apply_hybrid_options(
    query: &mut SelectQuery,
    options: &QueryOptions,
) -> Result<(), SqlError> {
    // Only create HybridSearch if weights are specified AND there's a vector search
    if options.vector_weight.is_none() && options.text_weight.is_none() {
        return Ok(());
    }

    // Must have vector search to use weights
    let vector_search = match query.vector_search.take() {
        Some(vs) => vs,
        None => {
            return Err(SqlError::syntax(
                "OPTION with weights requires a vector search (ORDER BY field <-> vector)",
            ));
        }
    };

    // Set defaults if not specified
    let vector_weight = options.vector_weight.unwrap_or(0.5);
    let text_weight = options.text_weight.unwrap_or(0.5);

    // Create hybrid search configuration
    query.hybrid_search = Some(HybridSearch {
        vector: vector_search,
        vector_weight,
        text_weight,
    });

    Ok(())
}

/// Result of parsing ORDER BY clause.
pub(super) struct OrderByResult {
    /// Standard ORDER BY clause.
    pub(super) order_by: Option<OrderBy>,
    /// Vector search extracted from ORDER BY field <-> 'vector'.
    pub(super) vector_search: Option<VectorSearch>,
}

pub(super) fn parse_order_by(
    order_by: &[OrderByExpr],
    limit: Option<usize>,
) -> Result<OrderByResult, SqlError> {
    if order_by.is_empty() {
        return Ok(OrderByResult {
            order_by: None,
            vector_search: None,
        });
    }

    // Check first expression for vector distance operator
    let first_expr = &order_by[0];
    if let Some(vector_search) = try_parse_vector_order_by(&first_expr.expr, limit)? {
        // Vector search found - additional ORDER BY columns not supported with vector search
        if order_by.len() > 1 {
            return Err(SqlError::unsupported(
                "Multiple ORDER BY columns are not supported with vector search",
            ));
        }
        return Ok(OrderByResult {
            order_by: None,
            vector_search: Some(vector_search),
        });
    }

    // Parse all ORDER BY columns
    let mut columns = Vec::with_capacity(order_by.len());
    for order_expr in order_by {
        let field = extract_identifier(&order_expr.expr)?;
        let direction = if order_expr.asc.unwrap_or(true) {
            SortDirection::Asc
        } else {
            SortDirection::Desc
        };
        columns.push(OrderByColumn { field, direction });
    }

    Ok(OrderByResult {
        order_by: Some(OrderBy { columns }),
        vector_search: None,
    })
}

/// Try to parse vector distance operator from ORDER BY expression.
fn try_parse_vector_order_by(
    expr: &Expr,
    limit: Option<usize>,
) -> Result<Option<VectorSearch>, SqlError> {
    // Vector distance uses the <-> (L2), <=> (Cosine), or <#> (IP) operators
    // e.g., embedding <-> '[0.1, 0.2, 0.3]'
    match expr {
        Expr::BinaryOp { left, op, right } => {
            // Check if this is a distance operator and get the metric
            let distance_metric = match get_vector_distance_metric(op) {
                Some(metric) => metric,
                None => return Ok(None),
            };

            let field = extract_identifier(left)?;
            let vector = extract_vector_value(right)?;

            // K is determined by LIMIT, default to 10
            let k = limit.unwrap_or(10);

            Ok(Some(VectorSearch {
                field,
                vector,
                k,
                distance_metric,
            }))
        }
        _ => Ok(None),
    }
}

/// Get the distance metric for a vector distance operator.
/// Returns Some(metric) for <-> (L2), <=> (Cosine), <#> (IP), None otherwise.
fn get_vector_distance_metric(op: &BinaryOperator) -> Option<DistanceMetric> {
    match op {
        // <=> is parsed as Spaceship by sqlparser-rs (SQL:2023 standard operator)
        BinaryOperator::Spaceship => Some(DistanceMetric::Cosine),
        BinaryOperator::Custom(op_str) => match op_str.as_str() {
            "<->" => Some(DistanceMetric::L2),
            "<#>" => Some(DistanceMetric::InnerProduct),
            _ => None,
        },
        BinaryOperator::PGCustomBinaryOperator(parts) => {
            // Custom operator like OPERATOR(<->), OPERATOR(<=>), OPERATOR(<#>)
            let op_str: String = parts.iter().map(|p| p.to_string()).collect();
            match op_str.as_str() {
                "<->" => Some(DistanceMetric::L2),
                "<=>" => Some(DistanceMetric::Cosine),
                "<#>" => Some(DistanceMetric::InnerProduct),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract a vector value from an expression (string representation).
fn extract_vector_value(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
        | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => Ok(s.clone()),
        Expr::Array(arr) => {
            // Handle array literal like [0.1, 0.2, 0.3]
            let elements: Vec<String> = arr
                .elem
                .iter()
                .map(|e| match e {
                    Expr::Value(sqlparser::ast::Value::Number(n, _)) => Ok(n.clone()),
                    _ => Err(SqlError::translation("Vector elements must be numbers")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", elements.join(",")))
        }
        _ => Err(SqlError::translation(format!(
            "Expected vector value (string or array), got: {expr:?}"
        ))),
    }
}

pub(super) fn parse_limit(
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
        Expr::Value(sqlparser::ast::Value::Number(n, _)) => n
            .parse()
            .map_err(|_| SqlError::translation(format!("Invalid LIMIT value: {n}"))),
        _ => Err(SqlError::translation("LIMIT value must be a number")),
    }
}

#[cfg(test)]
#[path = "options_tests.rs"]
mod tests;
