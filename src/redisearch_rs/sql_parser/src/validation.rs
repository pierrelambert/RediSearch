/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Basic validation for SQL queries.
//!
//! This module provides validation functions to check SQL queries
//! against supported features and constraints.

use crate::ast::{Condition, SelectQuery};
use crate::error::SqlError;

/// Maximum query size in bytes (64KB).
const MAX_QUERY_SIZE: usize = 65536;
/// Maximum nesting depth for boolean condition trees.
const MAX_NESTING_DEPTH: usize = 32;
/// Maximum number of values in a single IN clause.
const MAX_IN_VALUES: usize = 1000;
/// Maximum number of projected columns (explicit fields + aggregates).
const MAX_SELECT_COLUMNS: usize = 100;
/// Maximum number of ORDER BY columns.
const MAX_ORDER_BY_COLUMNS: usize = 8;

/// Validates the raw SQL query string before parsing.
pub fn validate_query_string(sql: &str) -> Result<(), SqlError> {
    if sql.is_empty() {
        return Err(SqlError::syntax("Empty query"));
    }

    if sql.len() > MAX_QUERY_SIZE {
        return Err(SqlError::syntax(format!(
            "Query exceeds maximum size of {} bytes",
            MAX_QUERY_SIZE
        )));
    }

    Ok(())
}

/// Validates a parsed SQL query against supported features.
pub fn validate_query(query: &SelectQuery) -> Result<(), SqlError> {
    validate_index_name(&query.index_name)?;

    let selected_columns = query.fields.len() + query.aggregates.len();
    if selected_columns > MAX_SELECT_COLUMNS {
        return Err(SqlError::translation(format!(
            "SELECT clause exceeds maximum of {} projected columns",
            MAX_SELECT_COLUMNS
        )));
    }

    if query
        .order_by
        .as_ref()
        .is_some_and(|order_by| order_by.columns.len() > MAX_ORDER_BY_COLUMNS)
    {
        return Err(SqlError::translation(format!(
            "ORDER BY clause exceeds maximum of {} columns",
            MAX_ORDER_BY_COLUMNS
        )));
    }

    for condition in &query.conditions {
        validate_condition(condition, 1)?;
    }

    if let Some(having) = &query.having {
        validate_condition(having, 1)?;
    }

    Ok(())
}

fn validate_index_name(index_name: &str) -> Result<(), SqlError> {
    if index_name.is_empty() {
        return Err(SqlError::translation("Index name cannot be empty"));
    }

    if index_name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(SqlError::translation(
            "Index name cannot contain whitespace or control characters",
        ));
    }

    Ok(())
}

fn validate_condition(condition: &Condition, depth: usize) -> Result<(), SqlError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(SqlError::translation(format!(
            "Query exceeds maximum nesting depth of {}",
            MAX_NESTING_DEPTH
        )));
    }

    match condition {
        Condition::In { values, .. } if values.len() > MAX_IN_VALUES => {
            Err(SqlError::translation(format!(
                "IN clause exceeds maximum of {} values",
                MAX_IN_VALUES
            )))
        }
        Condition::And(left, right) | Condition::Or(left, right) => {
            validate_condition(left, depth + 1)?;
            validate_condition(right, depth + 1)
        }
        Condition::Not(inner) => validate_condition(inner, depth + 1),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AggregateExpr, AggregateFunction, Condition, OrderBy, OrderByColumn, SelectField,
        SelectQuery, SortDirection, Value,
    };

    #[test]
    fn test_validate_empty_query() {
        let result = validate_query_string("");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_query() {
        let result = validate_query_string("SELECT * FROM idx");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_empty_index_name() {
        let query = SelectQuery::new("");
        let result = validate_query(&query);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_index_name() {
        let query = SelectQuery::new("my_index-123");
        let result = validate_query(&query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_index_name_with_colon() {
        let query = SelectQuery::new("idx:all");
        let result = validate_query(&query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_query_too_large() {
        // Create a query larger than MAX_QUERY_SIZE (64KB)
        let large_query = "x".repeat(MAX_QUERY_SIZE + 1);
        let result = validate_query_string(&large_query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("exceeds maximum size"));
    }

    #[test]
    fn test_validate_query_at_max_size() {
        // Query exactly at max size should be valid
        let max_query = "x".repeat(MAX_QUERY_SIZE);
        let result = validate_query_string(&max_query);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_index_name_with_control_character() {
        let query = SelectQuery::new("my\nindex");
        let result = validate_query(&query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("whitespace or control"));
    }

    #[test]
    fn test_validate_index_name_with_space() {
        let query = SelectQuery::new("my index");
        let result = validate_query(&query);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_select_columns_limit() {
        let mut query = SelectQuery::new("idx");
        query.fields = (0..=MAX_SELECT_COLUMNS)
            .map(|i| SelectField::new(format!("field_{i}")))
            .collect();

        let err = validate_query(&query).unwrap_err();
        assert!(err.message.contains("SELECT clause exceeds maximum"));
    }

    #[test]
    fn test_validate_select_columns_counts_aggregates() {
        let mut query = SelectQuery::new("idx");
        query.fields = (0..50)
            .map(|i| SelectField::new(format!("field_{i}")))
            .collect();
        query.aggregates = (0..51)
            .map(|_| AggregateExpr {
                function: AggregateFunction::Count,
                field: None,
                alias: None,
            })
            .collect();

        let err = validate_query(&query).unwrap_err();
        assert!(err.message.contains("SELECT clause exceeds maximum"));
    }

    #[test]
    fn test_validate_order_by_columns_limit() {
        let mut query = SelectQuery::new("idx");
        query.order_by = Some(OrderBy {
            columns: (0..=MAX_ORDER_BY_COLUMNS)
                .map(|i| OrderByColumn {
                    field: format!("field_{i}"),
                    direction: SortDirection::Asc,
                })
                .collect(),
        });

        let err = validate_query(&query).unwrap_err();
        assert!(err.message.contains("ORDER BY clause exceeds maximum"));
    }

    #[test]
    fn test_validate_in_clause_limit() {
        let mut query = SelectQuery::new("idx");
        query.conditions.push(Condition::In {
            field: "category".to_string(),
            values: (0..=MAX_IN_VALUES)
                .map(|i| Value::String(format!("value_{i}")))
                .collect(),
            negated: false,
        });

        let err = validate_query(&query).unwrap_err();
        assert!(err.message.contains("IN clause exceeds maximum"));
    }

    #[test]
    fn test_validate_nesting_depth_limit() {
        let mut condition = Condition::Equals {
            field: "category".to_string(),
            value: Value::String("electronics".to_string()),
        };

        for _ in 0..MAX_NESTING_DEPTH {
            condition = Condition::And(
                Box::new(condition),
                Box::new(Condition::Equals {
                    field: "status".to_string(),
                    value: Value::String("active".to_string()),
                }),
            );
        }

        let mut query = SelectQuery::new("idx");
        query.having = Some(condition);

        let err = validate_query(&query).unwrap_err();
        assert!(err.message.contains("maximum nesting depth"));
    }

    #[test]
    fn test_validate_query_with_limits_at_threshold() {
        let mut query = SelectQuery::new("idx");
        query.fields = (0..MAX_SELECT_COLUMNS)
            .map(|i| SelectField::new(format!("field_{i}")))
            .collect();
        query.order_by = Some(OrderBy {
            columns: (0..MAX_ORDER_BY_COLUMNS)
                .map(|i| OrderByColumn {
                    field: format!("field_{i}"),
                    direction: SortDirection::Asc,
                })
                .collect(),
        });
        query.conditions.push(Condition::In {
            field: "category".to_string(),
            values: (0..MAX_IN_VALUES)
                .map(|i| Value::String(format!("value_{i}")))
                .collect(),
            negated: false,
        });

        assert!(validate_query(&query).is_ok());
    }
}
