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

use crate::ast::SelectQuery;
use crate::error::SqlError;

/// Maximum query size in bytes (64KB).
const MAX_QUERY_SIZE: usize = 65536;

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
    // Validate index name is not empty
    if query.index_name.is_empty() {
        return Err(SqlError::translation("Index name cannot be empty"));
    }

    // Validate index name doesn't contain special characters
    if !query
        .index_name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(SqlError::translation(
            "Index name contains invalid characters",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SelectQuery;

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
        let query = SelectQuery {
            fields: vec![],
            index_name: "".to_string(),
            conditions: vec![],
            order_by: None,
            limit: None,
        };
        let result = validate_query(&query);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_valid_index_name() {
        let query = SelectQuery {
            fields: vec![],
            index_name: "my_index-123".to_string(),
            conditions: vec![],
            order_by: None,
            limit: None,
        };
        let result = validate_query(&query);
        assert!(result.is_ok());
    }
}

