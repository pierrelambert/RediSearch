/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use sqlparser::ast::{Distinct, Expr, SelectItem, SetExpr, Statement, TableFactor};

use crate::ast::{AggregateExpr, SelectField, SelectQuery};
use crate::error::SqlError;

use super::aggregates::{parse_aggregate_function, parse_group_by, parse_having_expression};
use super::expressions::{extract_identifier, parse_where_clause};
use super::options::{OrderByResult, parse_limit, parse_order_by};

pub(super) fn parse_statement(statement: Statement) -> Result<SelectQuery, SqlError> {
    match statement {
        Statement::Query(query) => {
            let select = match *query.body {
                SetExpr::Select(select) => select,
                _ => {
                    return Err(SqlError::unsupported("Only SELECT queries are supported"));
                }
            };

            // Check for DISTINCT
            let distinct = matches!(select.distinct, Some(Distinct::Distinct));
            if matches!(select.distinct, Some(Distinct::On(_))) {
                return Err(SqlError::unsupported(
                    "DISTINCT ON is not supported, use DISTINCT instead",
                ));
            }

            // Parse fields and aggregates from SELECT clause
            let SelectItemsResult { fields, aggregates } = parse_select_items(&select.projection)?;

            // Parse FROM clause
            let index_name = parse_from_clause(&select.from)?;

            // Parse WHERE clause
            let conditions = if let Some(selection) = select.selection {
                parse_where_clause(&selection)?
            } else {
                Vec::new()
            };

            // Parse GROUP BY clause
            let group_by = parse_group_by(&select.group_by)?;

            // Parse HAVING clause
            let having = if let Some(having_expr) = &select.having {
                Some(parse_having_expression(having_expr)?)
            } else {
                None
            };

            // Parse LIMIT/OFFSET clause first (needed for vector K)
            let limit = parse_limit(query.limit.as_ref(), query.offset.as_ref())?;

            // Parse ORDER BY clause (may contain vector search)
            let OrderByResult {
                order_by,
                vector_search,
            } = if let Some(ref ob) = query.order_by {
                parse_order_by(&ob.exprs, limit.as_ref().map(|l| l.count as usize))?
            } else {
                OrderByResult {
                    order_by: None,
                    vector_search: None,
                }
            };

            Ok(SelectQuery {
                fields,
                index_name,
                conditions,
                order_by,
                limit,
                distinct,
                aggregates,
                group_by,
                having,
                vector_search,
                hybrid_search: None, // Hybrid search is configured via OPTION clause.
            })
        }
        _ => Err(SqlError::unsupported(
            "Only SELECT statements are supported",
        )),
    }
}

/// Result of parsing SELECT items.
struct SelectItemsResult {
    fields: Vec<SelectField>,
    aggregates: Vec<AggregateExpr>,
}

fn parse_select_items(items: &[SelectItem]) -> Result<SelectItemsResult, SqlError> {
    let mut fields = Vec::new();
    let mut aggregates = Vec::new();

    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                // SELECT * - return empty vec to indicate all fields
                return Ok(SelectItemsResult {
                    fields: Vec::new(),
                    aggregates: Vec::new(),
                });
            }
            SelectItem::UnnamedExpr(Expr::Function(func)) => {
                if let Some(agg) = parse_aggregate_function(func, None)? {
                    aggregates.push(agg);
                } else {
                    return Err(SqlError::unsupported(format!(
                        "Unsupported function in SELECT: {}",
                        func.name
                    )));
                }
            }
            SelectItem::UnnamedExpr(expr) => {
                let field_name = extract_identifier(expr)?;
                fields.push(SelectField {
                    name: field_name,
                    alias: None,
                });
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Function(func),
                alias,
            } => {
                if let Some(agg) = parse_aggregate_function(func, Some(alias.value.clone()))? {
                    aggregates.push(agg);
                } else {
                    return Err(SqlError::unsupported(format!(
                        "Unsupported function in SELECT: {}",
                        func.name
                    )));
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                let field_name = extract_identifier(expr)?;
                fields.push(SelectField {
                    name: field_name,
                    alias: Some(alias.value.clone()),
                });
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "Unsupported SELECT item: {item:?}"
                )));
            }
        }
    }

    Ok(SelectItemsResult { fields, aggregates })
}

fn normalize_relation_name(name: &str) -> String {
    name.strip_prefix('"')
        .and_then(|trimmed| trimmed.strip_suffix('"'))
        .unwrap_or(name)
        .to_string()
}

fn parse_from_clause(from: &[sqlparser::ast::TableWithJoins]) -> Result<String, SqlError> {
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
        TableFactor::Table { name, .. } => Ok(normalize_relation_name(&name.to_string())),
        _ => Err(SqlError::unsupported(
            "Only simple table references are supported",
        )),
    }
}

#[cfg(test)]
#[path = "select_tests.rs"]
mod tests;
