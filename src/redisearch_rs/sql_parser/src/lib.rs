/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL Parser for RediSearch.
//!
//! This crate provides SQL to RQL (RediSearch Query Language) translation.
//! It parses standard SQL SELECT queries and converts them into equivalent
//! RediSearch commands.
//!
//! # Phase 1 Support
//!
//! The following SQL constructs are supported:
//!
//! ## SELECT clause
//! - `SELECT *` - Returns all indexed fields
//! - `SELECT field1, field2` - Explicit field selection
//!
//! ## FROM clause
//! - `FROM idx` - Index name (required)
//!
//! ## WHERE clause
//! - `field = 'value'` - Exact match
//! - `field > 100` - Greater than
//! - `field >= 100` - Greater than or equal
//! - `field < 100` - Less than
//! - `field <= 100` - Less than or equal
//! - `field BETWEEN a AND b` - Inclusive range
//!
//! ## ORDER BY clause
//! - `ORDER BY field ASC` - Ascending sort
//! - `ORDER BY field DESC` - Descending sort
//!
//! ## LIMIT clause
//! - `LIMIT n` - First n results
//! - `LIMIT n OFFSET m` - Skip m, take n
//!
//! # Example
//!
//! ```
//! use sql_parser::translate;
//!
//! let result = translate("SELECT * FROM products WHERE price > 100").unwrap();
//! assert_eq!(result.index_name, "products");
//! assert_eq!(result.query_string, "@price:[(100 +inf]");
//! ```

pub mod ast;
pub mod cache;
pub mod error;
pub mod parser;
pub mod translator;
pub mod validation;

pub use cache::{
    CacheConfig, CacheStats, clear_cache, get_cache_stats, set_cache_config, translate_cached,
};
pub use error::SqlError;

/// The type of Redis command to use for the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Use FT.SEARCH for this query.
    Search,
    /// Use FT.AGGREGATE for this query (for GROUP BY).
    Aggregate,
}

/// Result of translating a SQL query to RQL.
#[derive(Debug, Clone)]
pub struct Translation {
    /// The Redis command to use.
    pub command: Command,
    /// The index name from the FROM clause.
    pub index_name: String,
    /// The RQL query string (e.g., "@field:value").
    pub query_string: String,
    /// Additional arguments (RETURN, SORTBY, LIMIT, etc.).
    pub arguments: Vec<String>,
}

/// Translates a SQL query string to RQL format.
///
/// This is the main entry point for SQL translation. It parses the SQL
/// query, validates it, and produces a [`Translation`] that can be used
/// to execute the equivalent RediSearch command.
///
/// # Arguments
///
/// * `sql` - A SQL query string to translate.
///
/// # Returns
///
/// A [`Translation`] containing the RQL equivalent, or a [`SqlError`]
/// if the query is invalid or unsupported.
///
/// # Example
///
/// ```
/// use sql_parser::{translate, Command};
///
/// let result = translate("SELECT name, price FROM products LIMIT 10").unwrap();
/// assert_eq!(result.command, Command::Search);
/// assert_eq!(result.index_name, "products");
/// assert!(result.arguments.contains(&"RETURN".to_string()));
/// assert!(result.arguments.contains(&"LIMIT".to_string()));
/// ```
pub fn translate(sql: &str) -> Result<Translation, SqlError> {
    // Validate input
    validation::validate_query_string(sql)?;

    // Parse SQL
    let query = parser::parse(sql)?;

    // Validate parsed query
    validation::validate_query(&query)?;

    // Translate to RQL
    translator::translate(query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_star() {
        let result = translate("SELECT * FROM products").unwrap();
        assert_eq!(result.index_name, "products");
        assert_eq!(result.query_string, "*");
        assert!(result.arguments.is_empty());
    }

    #[test]
    fn test_where_equality() {
        let result = translate("SELECT * FROM idx WHERE status = 'active'").unwrap();
        assert_eq!(result.query_string, "@status:active");
    }

    #[test]
    fn test_where_range() {
        let result = translate("SELECT * FROM idx WHERE price > 100").unwrap();
        assert_eq!(result.query_string, "@price:[(100 +inf]");
    }

    #[test]
    fn test_between() {
        let result = translate("SELECT * FROM idx WHERE price BETWEEN 10 AND 100").unwrap();
        assert_eq!(result.query_string, "@price:[10 100]");
    }

    #[test]
    fn test_order_by() {
        let result = translate("SELECT * FROM idx ORDER BY price DESC").unwrap();
        assert!(result.arguments.contains(&"SORTBY".to_string()));
        assert!(result.arguments.contains(&"price".to_string()));
        assert!(result.arguments.contains(&"DESC".to_string()));
    }

    #[test]
    fn test_limit() {
        let result = translate("SELECT * FROM idx LIMIT 20").unwrap();
        assert!(result.arguments.contains(&"LIMIT".to_string()));
        assert!(result.arguments.contains(&"0".to_string())); // offset
        assert!(result.arguments.contains(&"20".to_string())); // count
    }

    #[test]
    fn test_select_fields() {
        let result = translate("SELECT name, price FROM products").unwrap();
        assert!(result.arguments.contains(&"RETURN".to_string()));
        assert!(result.arguments.contains(&"2".to_string()));
        assert!(result.arguments.contains(&"name".to_string()));
        assert!(result.arguments.contains(&"price".to_string()));
    }

    #[test]
    fn test_syntax_error() {
        let result = translate("SELEC * FROM idx");
        assert!(result.is_err());
    }

    #[test]
    fn test_command_is_search() {
        let result = translate("SELECT * FROM idx").unwrap();
        assert_eq!(result.command, Command::Search);
    }

    #[test]
    fn test_limit_with_offset() {
        let result = translate("SELECT * FROM idx LIMIT 10 OFFSET 5").unwrap();
        let limit_idx = result.arguments.iter().position(|a| a == "LIMIT").unwrap();
        assert_eq!(result.arguments[limit_idx + 1], "5"); // offset
        assert_eq!(result.arguments[limit_idx + 2], "10"); // count
    }

    #[test]
    fn test_less_than_or_equal() {
        let result = translate("SELECT * FROM idx WHERE price <= 100").unwrap();
        assert_eq!(result.query_string, "@price:[-inf 100]");
    }

    #[test]
    fn test_greater_than_or_equal() {
        let result = translate("SELECT * FROM idx WHERE price >= 50").unwrap();
        assert_eq!(result.query_string, "@price:[50 +inf]");
    }

    #[test]
    fn test_less_than() {
        let result = translate("SELECT * FROM idx WHERE price < 100").unwrap();
        assert_eq!(result.query_string, "@price:[-inf (100]");
    }
}

