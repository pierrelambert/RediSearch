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
//! # Supported SQL Features
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
//! - `field IN ('a', 'b', 'c')` - Match any value (strings use TAG syntax)
//! - `field NOT IN ('a', 'b')` - Exclude values
//!
//! ## ORDER BY clause
//! - `ORDER BY field ASC` - Ascending sort
//! - `ORDER BY field DESC` - Descending sort
//! - `ORDER BY field <-> '[...]'` - Vector L2/Euclidean distance (pgvector syntax)
//! - `ORDER BY field <=> '[...]'` - Vector Cosine distance (pgvector syntax)
//! - `ORDER BY field <#> '[...]'` - Vector Inner Product (pgvector syntax)
//!
//! ## LIMIT clause
//! - `LIMIT n` - First n results
//! - `LIMIT n OFFSET m` - Skip m, take n
//!
//! ## Vector Search (pgvector syntax)
//!
//! Vector similarity search supports three distance metrics:
//!
//! | Operator | Distance Metric | Description |
//! |----------|----------------|-------------|
//! | `<->`    | L2 (Euclidean) | Squared Euclidean distance |
//! | `<=>`    | Cosine         | 1 - cosine similarity |
//! | `<#>`    | Inner Product  | Negative inner product |
//!
//! ```sql
//! -- L2 distance search (pgvector default)
//! SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10
//!
//! -- Cosine distance search
//! SELECT * FROM products ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10
//!
//! -- Inner product search
//! SELECT * FROM products ORDER BY embedding <#> '[0.1, 0.2, 0.3]' LIMIT 10
//!
//! -- Filter + KNN (works with any distance metric)
//! SELECT * FROM products
//! WHERE category = 'electronics'
//! ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5
//! ```
//!
//! **Note:** RediSearch uses the distance metric specified during index creation
//! (via `DISTANCE_METRIC` in `FT.CREATE`). The SQL operator indicates intent but
//! the actual metric used depends on the index configuration.
//!
//! This translates to RediSearch KNN queries:
//! - Pure KNN: `FT.SEARCH idx "*=>[KNN 10 @embedding $BLOB]" PARAMS 2 BLOB <vector>`
//! - With filter: `FT.SEARCH idx "@category:{electronics}=>[KNN 5 @embedding $BLOB]" ...`
//!
//! The `K` value comes from the `LIMIT` clause (defaults to 10 if not specified).
//!
//! ## FT.HYBRID with Weighted Scoring
//!
//! For hybrid search with weighted scoring between vector and text results,
//! use the `OPTION` clause:
//!
//! ```sql
//! -- Weighted hybrid search (70% vector, 30% text)
//! SELECT * FROM products
//! WHERE name MATCH 'laptop'
//! ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10
//! OPTION (vector_weight = 0.7, text_weight = 0.3)
//! ```
//!
//! This translates to:
//! ```text
//! FT.HYBRID products "@name:laptop"
//!   VECTOR embedding K 10 VECTOR_BLOB <blob>
//!   WEIGHT 0.7 TEXT 0.3
//! ```
//!
//! **OPTION clause parameters:**
//! - `vector_weight`: Weight for vector similarity (0.0 to 1.0, default: 0.5)
//! - `text_weight`: Weight for text relevance (0.0 to 1.0, default: 0.5)
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
    /// Use FT.HYBRID for weighted vector + text search.
    Hybrid,
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
    fn test_where_string_equality() {
        let result = translate("SELECT * FROM idx WHERE status = 'active'").unwrap();
        // String equality uses TAG syntax with curly braces for exact match
        assert_eq!(result.query_string, "@status:{active}");
    }

    #[test]
    fn test_where_numeric_equality() {
        let result = translate("SELECT * FROM idx WHERE count = 42").unwrap();
        // Numeric equality uses range syntax [n n]
        assert_eq!(result.query_string, "@count:[42 42]");
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

    // IN clause integration tests
    #[test]
    fn test_in_string_values() {
        let result =
            translate("SELECT * FROM products WHERE category IN ('electronics', 'accessories')")
                .unwrap();
        assert_eq!(result.query_string, "@category:{electronics|accessories}");
    }

    #[test]
    fn test_not_in_string_values() {
        let result =
            translate("SELECT * FROM products WHERE status NOT IN ('deleted', 'archived')")
                .unwrap();
        assert_eq!(result.query_string, "-@status:{deleted|archived}");
    }

    #[test]
    fn test_in_numeric_values() {
        let result = translate("SELECT * FROM products WHERE price IN (10, 20, 30)").unwrap();
        assert_eq!(
            result.query_string,
            "(@price:[10 10]|@price:[20 20]|@price:[30 30])"
        );
    }

    #[test]
    fn test_not_in_numeric_values() {
        let result = translate("SELECT * FROM products WHERE count NOT IN (0, 1)").unwrap();
        assert_eq!(result.query_string, "-(@count:[0 0]|@count:[1 1])");
    }

    #[test]
    fn test_in_combined_with_other_conditions() {
        let result = translate("SELECT * FROM products WHERE category IN ('electronics', 'accessories') AND price > 100").unwrap();
        assert_eq!(
            result.query_string,
            "@category:{electronics|accessories} @price:[(100 +inf]"
        );
    }

    // New features integration tests
    #[test]
    fn test_not_equals() {
        let result = translate("SELECT * FROM idx WHERE status != 'deleted'").unwrap();
        assert_eq!(result.query_string, "-@status:{deleted}");
    }

    #[test]
    fn test_or_operator() {
        let result = translate("SELECT * FROM idx WHERE a = 1 OR b = 2").unwrap();
        // RQL OR requires spaces around pipe between parenthesized sub-conditions
        assert_eq!(result.query_string, "(@a:[1 1]) | (@b:[2 2])");
    }

    #[test]
    fn test_like_prefix() {
        let result = translate("SELECT * FROM idx WHERE name LIKE 'Lap%'").unwrap();
        assert_eq!(result.query_string, "@name:Lap*");
    }

    #[test]
    fn test_like_suffix() {
        let result = translate("SELECT * FROM idx WHERE name LIKE '%top'").unwrap();
        assert_eq!(result.query_string, "@name:*top");
    }

    #[test]
    fn test_like_contains() {
        let result = translate("SELECT * FROM idx WHERE name LIKE '%apt%'").unwrap();
        assert_eq!(result.query_string, "@name:*apt*");
    }

    #[test]
    fn test_select_with_alias() {
        let result = translate("SELECT name AS product_name FROM products").unwrap();
        assert!(result.arguments.contains(&"RETURN".to_string()));
        assert!(result.arguments.contains(&"AS".to_string()));
        assert!(result.arguments.contains(&"product_name".to_string()));
    }

    #[test]
    fn test_is_null() {
        let result = translate("SELECT * FROM idx WHERE category IS NULL").unwrap();
        assert_eq!(result.query_string, "ismissing(@category)");
    }

    #[test]
    fn test_is_not_null() {
        let result = translate("SELECT * FROM idx WHERE category IS NOT NULL").unwrap();
        assert_eq!(result.query_string, "-ismissing(@category)");
    }

    #[test]
    fn test_select_distinct() {
        let result = translate("SELECT DISTINCT category FROM idx").unwrap();
        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"GROUPBY".to_string()));
    }

    #[test]
    fn test_count_star() {
        let result = translate("SELECT COUNT(*) FROM idx").unwrap();
        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"REDUCE".to_string()));
        assert!(result.arguments.contains(&"COUNT".to_string()));
    }

    #[test]
    fn test_sum_aggregate() {
        let result = translate("SELECT SUM(price) FROM idx").unwrap();
        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"REDUCE".to_string()));
        assert!(result.arguments.contains(&"SUM".to_string()));
    }

    #[test]
    fn test_group_by() {
        let result = translate("SELECT category, COUNT(*) FROM idx GROUP BY category").unwrap();
        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"GROUPBY".to_string()));
        assert!(result.arguments.contains(&"@category".to_string()));
    }

    #[test]
    fn test_group_by_with_having() {
        let result =
            translate("SELECT category, COUNT(*) FROM idx GROUP BY category HAVING COUNT(*) > 5")
                .unwrap();
        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"FILTER".to_string()));
    }

    #[test]
    fn test_having_with_alias() {
        // When SELECT uses an alias for COUNT(*), HAVING should reference that alias
        let result = translate(
            "SELECT category, COUNT(*) as cnt FROM idx GROUP BY category HAVING COUNT(*) > 3",
        )
        .unwrap();

        assert_eq!(result.command, Command::Aggregate);
        assert!(result.arguments.contains(&"FILTER".to_string()));
        // The FILTER should reference @cnt (the alias), not @count (the default name)
        let filter_idx = result
            .arguments
            .iter()
            .position(|a| a == "FILTER")
            .expect("FILTER should be present");
        let filter_expr = &result.arguments[filter_idx + 1];
        assert_eq!(
            filter_expr, "@cnt>3",
            "HAVING should reference the alias 'cnt', not 'count'"
        );
    }

    #[test]
    fn test_having_without_alias() {
        // When SELECT has no alias, HAVING should use the default name
        let result =
            translate("SELECT category, COUNT(*) FROM idx GROUP BY category HAVING COUNT(*) > 5")
                .unwrap();

        assert_eq!(result.command, Command::Aggregate);
        let filter_idx = result
            .arguments
            .iter()
            .position(|a| a == "FILTER")
            .expect("FILTER should be present");
        let filter_expr = &result.arguments[filter_idx + 1];
        assert_eq!(
            filter_expr, "@count>5",
            "HAVING should reference the default name 'count'"
        );
    }

    #[test]
    fn test_having_with_sum_alias() {
        // Test HAVING with SUM and an alias
        let result = translate(
            "SELECT category, SUM(price) as total FROM idx GROUP BY category HAVING SUM(price) > 100",
        )
        .unwrap();

        assert_eq!(result.command, Command::Aggregate);
        let filter_idx = result
            .arguments
            .iter()
            .position(|a| a == "FILTER")
            .expect("FILTER should be present");
        let filter_expr = &result.arguments[filter_idx + 1];
        assert_eq!(
            filter_expr, "@total>100",
            "HAVING should reference the alias 'total', not 'sum_price'"
        );
    }

    // Vector search tests - note: <-> operator parsing depends on sqlparser support
    // These tests verify the AST and translation when vector search is present
    #[test]
    fn test_vector_knn_query_string() {
        // Test translation of a query with vector search set programmatically
        use crate::ast::{DistanceMetric, SelectQuery, VectorSearch};
        use crate::translator;

        let mut query = SelectQuery::new("products");
        query.vector_search = Some(VectorSearch {
            field: "embedding".to_string(),
            vector: "[0.1, 0.2, 0.3]".to_string(),
            k: 10,
            distance_metric: DistanceMetric::default(),
        });

        let result = translator::translate(query).unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
        assert!(result.arguments.contains(&"PARAMS".to_string()));
        assert!(result.arguments.contains(&"BLOB".to_string()));
    }

    #[test]
    fn test_hybrid_vector_search() {
        // Test hybrid search with filter + vector
        use crate::ast::{Condition, DistanceMetric, SelectQuery, Value, VectorSearch};
        use crate::translator;

        let mut query = SelectQuery::new("products");
        query.conditions.push(Condition::Equals {
            field: "category".to_string(),
            value: Value::String("electronics".to_string()),
        });
        query.vector_search = Some(VectorSearch {
            field: "embedding".to_string(),
            vector: "[0.1, 0.2]".to_string(),
            k: 5,
            distance_metric: DistanceMetric::default(),
        });

        let result = translator::translate(query).unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(
            result.query_string,
            "@category:{electronics}=>[KNN 5 @embedding $BLOB]"
        );
    }

    #[test]
    fn test_ft_hybrid_command() {
        // Test FT.HYBRID command generation
        use crate::ast::{DistanceMetric, HybridSearch, SelectQuery, VectorSearch};
        use crate::translator;

        let mut query = SelectQuery::new("products");
        query.hybrid_search = Some(HybridSearch {
            vector: VectorSearch {
                field: "embedding".to_string(),
                vector: "[0.1, 0.2, 0.3]".to_string(),
                k: 10,
                distance_metric: DistanceMetric::default(),
            },
            vector_weight: 0.7,
            text_weight: 0.3,
        });

        let result = translator::translate(query).unwrap();
        assert_eq!(result.command, Command::Hybrid);
        assert!(result.arguments.contains(&"VECTOR".to_string()));
        assert!(result.arguments.contains(&"WEIGHT".to_string()));
    }

    // End-to-end SQL parsing tests with <-> operator (pgvector syntax)
    #[test]
    fn test_vector_search_sql_pure_knn() {
        // Pure KNN search: SELECT * FROM products ORDER BY embedding <-> '[...]' LIMIT 10
        let result =
            translate("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(result.index_name, "products");
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
        // Verify PARAMS include vector blob
        assert!(result.arguments.contains(&"PARAMS".to_string()));
        assert!(result.arguments.contains(&"2".to_string())); // PARAMS count
        assert!(result.arguments.contains(&"BLOB".to_string()));
        assert!(result.arguments.contains(&"[0.1, 0.2, 0.3]".to_string()));
    }

    #[test]
    fn test_vector_search_sql_with_filter() {
        // Hybrid: Filter + KNN
        let result = translate(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 5",
        )
        .unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(
            result.query_string,
            "@category:{electronics}=>[KNN 5 @embedding $BLOB]"
        );
    }

    #[test]
    fn test_vector_search_sql_default_k() {
        // Without LIMIT, K defaults to 10
        let result =
            translate("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]'").unwrap();
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
    }

    #[test]
    fn test_vector_search_sql_with_fields() {
        // Vector search with specific field selection
        let result = translate(
            "SELECT name, price FROM products ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 5",
        )
        .unwrap();
        assert_eq!(result.query_string, "*=>[KNN 5 @embedding $BLOB]");
        // RETURN should be present
        assert!(result.arguments.contains(&"RETURN".to_string()));
        assert!(result.arguments.contains(&"name".to_string()));
        assert!(result.arguments.contains(&"price".to_string()));
    }

    #[test]
    fn test_vector_search_sql_with_multiple_filters() {
        // Vector search with multiple WHERE conditions
        let result = translate(
            "SELECT * FROM products WHERE category = 'electronics' AND price > 100 ORDER BY embedding <-> '[0.5]' LIMIT 3",
        )
        .unwrap();
        assert_eq!(
            result.query_string,
            "@category:{electronics} @price:[(100 +inf]=>[KNN 3 @embedding $BLOB]"
        );
    }

    // Cosine distance tests (<=> operator)
    #[test]
    fn test_vector_search_sql_cosine_distance() {
        // Cosine distance: SELECT * FROM products ORDER BY embedding <=> '[...]' LIMIT 10
        let result =
            translate("SELECT * FROM products ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(result.index_name, "products");
        // KNN syntax is the same regardless of distance metric (metric is set at index creation)
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
        assert!(result.arguments.contains(&"PARAMS".to_string()));
        assert!(result.arguments.contains(&"[0.1, 0.2, 0.3]".to_string()));
    }

    #[test]
    fn test_vector_search_sql_cosine_with_filter() {
        // Cosine with filter: Filter + KNN with <=> operator
        let result = translate(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5",
        )
        .unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(
            result.query_string,
            "@category:{electronics}=>[KNN 5 @embedding $BLOB]"
        );
    }

    #[test]
    fn test_vector_search_sql_inner_product() {
        // Inner product: SELECT * FROM products ORDER BY embedding <#> '[...]' LIMIT 10
        let result =
            translate("SELECT * FROM products ORDER BY embedding <#> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert_eq!(result.command, Command::Search);
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
        assert!(result.arguments.contains(&"PARAMS".to_string()));
    }

    // FT.HYBRID tests with OPTION clause
    #[test]
    fn test_hybrid_search_sql_basic() {
        // FT.HYBRID with weights via OPTION clause
        let result = translate(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10 \
             OPTION (vector_weight = 0.7, text_weight = 0.3)",
        )
        .unwrap();

        assert_eq!(result.command, Command::Hybrid);
        assert_eq!(result.index_name, "products");
        // For FT.HYBRID, query_string is just the text query (no KNN syntax)
        assert_eq!(result.query_string, "*");

        // Arguments should include VECTOR, K, VECTOR_BLOB, WEIGHT, TEXT
        assert!(result.arguments.contains(&"VECTOR".to_string()));
        assert!(result.arguments.contains(&"embedding".to_string()));
        assert!(result.arguments.contains(&"K".to_string()));
        assert!(result.arguments.contains(&"10".to_string()));
        assert!(result.arguments.contains(&"VECTOR_BLOB".to_string()));
        assert!(result.arguments.contains(&"WEIGHT".to_string()));
        assert!(result.arguments.contains(&"0.7".to_string()));
        assert!(result.arguments.contains(&"TEXT".to_string()));
        assert!(result.arguments.contains(&"0.3".to_string()));
    }

    #[test]
    fn test_hybrid_search_sql_with_filter() {
        // FT.HYBRID with filter and weights
        let result = translate(
            "SELECT * FROM products \
             WHERE category = 'electronics' \
             ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5 \
             OPTION (vector_weight = 0.6, text_weight = 0.4)",
        )
        .unwrap();

        assert_eq!(result.command, Command::Hybrid);
        // Query string includes the filter
        assert_eq!(result.query_string, "@category:{electronics}");

        // Verify weights
        assert!(result.arguments.contains(&"WEIGHT".to_string()));
        assert!(result.arguments.contains(&"0.6".to_string()));
        assert!(result.arguments.contains(&"TEXT".to_string()));
        assert!(result.arguments.contains(&"0.4".to_string()));
    }

    #[test]
    fn test_hybrid_search_sql_with_limit() {
        // FT.HYBRID includes LIMIT
        let result = translate(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1]' LIMIT 20 OFFSET 10 \
             OPTION (vector_weight = 0.5, text_weight = 0.5)",
        )
        .unwrap();

        assert_eq!(result.command, Command::Hybrid);
        // LIMIT should be in arguments
        assert!(result.arguments.contains(&"LIMIT".to_string()));
        assert!(result.arguments.contains(&"10".to_string())); // offset
        assert!(result.arguments.contains(&"20".to_string())); // count
    }

    // NOT operator integration tests
    #[test]
    fn test_not_equals_integration() {
        let result = translate("SELECT * FROM idx WHERE NOT (category = 'electronics')").unwrap();
        assert_eq!(result.query_string, "-(@category:{electronics})");
    }

    #[test]
    fn test_not_greater_than_integration() {
        let result = translate("SELECT * FROM idx WHERE NOT (price > 100)").unwrap();
        assert_eq!(result.query_string, "-(@price:[(100 +inf])");
    }

    #[test]
    fn test_not_like_integration() {
        let result = translate("SELECT * FROM idx WHERE NOT (name LIKE 'Lap%')").unwrap();
        assert_eq!(result.query_string, "-(@name:Lap*)");
    }

    #[test]
    fn test_not_combined_with_and() {
        let result =
            translate("SELECT * FROM idx WHERE NOT (category = 'electronics') AND price > 50")
                .unwrap();
        assert_eq!(
            result.query_string,
            "-(@category:{electronics}) @price:[(50 +inf]"
        );
    }

    // Multiple ORDER BY integration tests
    // FT.SEARCH only supports single ORDER BY column
    #[test]
    fn test_order_by_multiple_columns_search_rejected() {
        let result = translate("SELECT * FROM products ORDER BY category ASC, price DESC");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Multiple ORDER BY columns are not supported by FT.SEARCH")
        );
    }

    #[test]
    fn test_order_by_three_columns_search_rejected() {
        let result = translate("SELECT * FROM idx ORDER BY category ASC, price DESC, name ASC");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Multiple ORDER BY columns are not supported by FT.SEARCH")
        );
    }

    #[test]
    fn test_order_by_multiple_with_where_and_limit_search_rejected() {
        let result = translate(
            "SELECT * FROM products WHERE status = 'active' ORDER BY price DESC, name ASC LIMIT 10",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Multiple ORDER BY columns are not supported by FT.SEARCH")
        );
    }

    // FT.AGGREGATE supports multiple ORDER BY columns
    #[test]
    fn test_order_by_multiple_columns_aggregate_integration() {
        let result = translate(
            "SELECT category, COUNT(*) AS cnt FROM products GROUP BY category ORDER BY category ASC, cnt DESC",
        )
        .unwrap();
        assert_eq!(result.command, Command::Aggregate);
        let args = result.arguments;
        assert!(args.contains(&"SORTBY".to_string()));
        // SORTBY nargs @field1 ASC @field2 DESC
        let sortby_idx = args.iter().position(|x| x == "SORTBY").unwrap();
        assert_eq!(args[sortby_idx + 1], "4"); // nargs = 2 columns * 2
        assert_eq!(args[sortby_idx + 2], "@category");
        assert_eq!(args[sortby_idx + 3], "ASC");
        assert_eq!(args[sortby_idx + 4], "@cnt");
        assert_eq!(args[sortby_idx + 5], "DESC");
    }
}

#[cfg(test)]
mod debug_test;
