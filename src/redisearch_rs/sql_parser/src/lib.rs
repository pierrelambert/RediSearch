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
//! For hybrid search with weighted scoring between vector and text results,
//! use the `OPTION` clause:
//!
//! ```sql
//! -- Weighted hybrid search (70% vector, 30% text)
//! SELECT * FROM products
//! WHERE category = 'electronics'
//! ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10
//! OPTION (vector_weight = 0.7, text_weight = 0.3)
//! ```
//!
//! This translates to:
//! ```text
//! FT.HYBRID products SEARCH "@category:{electronics}"
//!   VSIM @embedding $BLOB KNN 2 K 10
//!   COMBINE LINEAR 4 ALPHA 0.7 BETA 0.3
//!   LIMIT 0 10 PARAMS 2 BLOB <blob>
//! ```
//!
//! **OPTION clause parameters:**
//! - `vector_weight`: Weight for vector similarity (0.0 to 1.0, default: 0.5)
//! - `text_weight`: Weight for text relevance (0.0 to 1.0, default: 0.5)
//!
//! # Schema-aware vs Schema-unaware
//! [`translate_with_schema`] is the primary API when field metadata is available.
//! It validates schema-dependent constraints before producing a [`Translation`].
//!
//! [`translate`] is a convenience wrapper for schema-unaware callers. It uses an
//! empty [`QuerySchema`], so it parses and translates SQL without validating
//! field types or other schema-dependent constraints.
//!
//! # Example
//! ```
//! use sql_parser::{translate_with_schema, Command, FieldCapabilities, QuerySchema};
//!
//! let schema = QuerySchema::new(1)
//!     .with_field("category", FieldCapabilities::tag());
//! let result = translate_with_schema(
//!     "SELECT * FROM products WHERE category = 'electronics'",
//!     &schema,
//! )
//! .unwrap();
//!
//! assert_eq!(result.command, Command::Search);
//! assert_eq!(result.index_name, "products");
//! assert_eq!(result.query_string, "@category:{electronics}");
//! ```

pub mod ast;
pub mod cache;
pub mod error;
pub mod parser;
pub mod translator;
pub mod validation;

pub use cache::{
    CacheConfig, CacheStats, clear_cache, get_cache_stats, set_cache_config, translate_cached,
    translate_cached_with_schema,
};
pub use error::SqlError;

use std::collections::HashMap;

/// Capabilities of a field for query translation and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldCapabilities {
    /// Whether the field supports TAG-style exact matching.
    pub supports_tag_queries: bool,
    /// Whether the field is indexed as TEXT.
    pub supports_text_queries: bool,
    /// Whether the field indexes missing values for `IS NULL` / `IS NOT NULL`.
    pub supports_index_missing: bool,
}

impl FieldCapabilities {
    /// Capabilities for a TAG field.
    #[must_use]
    pub const fn tag() -> Self {
        Self {
            supports_tag_queries: true,
            supports_text_queries: false,
            supports_index_missing: false,
        }
    }

    /// Capabilities for a TEXT field.
    #[must_use]
    pub const fn text() -> Self {
        Self {
            supports_tag_queries: false,
            supports_text_queries: true,
            supports_index_missing: false,
        }
    }
}

/// Schema metadata used to validate and cache SQL translations.
#[derive(Debug, Clone, Default)]
pub struct QuerySchema {
    /// Monotonically increasing schema or index revision.
    pub version: u64,
    /// Field capabilities keyed by field name.
    pub fields: HashMap<String, FieldCapabilities>,
}

impl QuerySchema {
    /// Create an empty schema with the given version.
    #[must_use]
    pub fn new(version: u64) -> Self {
        Self {
            version,
            fields: HashMap::new(),
        }
    }

    /// Add or update capabilities for a field.
    #[must_use]
    pub fn with_field(mut self, field: impl Into<String>, capabilities: FieldCapabilities) -> Self {
        self.fields
            .insert(field.into().to_ascii_lowercase(), capabilities);
        self
    }

    /// Return capabilities for a field, if schema metadata is available.
    #[must_use]
    pub fn field_capabilities(&self, field: &str) -> Option<FieldCapabilities> {
        self.fields.get(&field.to_ascii_lowercase()).copied()
    }
}

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

/// Translates a SQL query string to RQL format without schema metadata.
///
/// This convenience wrapper calls [`translate_with_schema`] with an empty
/// [`QuerySchema`]. It parses the SQL query, validates query structure, and
/// produces a [`Translation`] that can be used to execute the equivalent
/// RediSearch command.
///
/// Because this function is schema-unaware, it does not validate field types
/// or other schema-dependent constraints. Prefer [`translate_with_schema`]
/// when schema metadata is available.
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
    translate_with_schema(sql, &QuerySchema::default())
}

/// Translates a SQL query string to RQL format using schema metadata.
///
/// This is the primary API when field metadata is available. It allows
/// providing field capability information so the translator can validate
/// schema-dependent constraints before generating RQL.
///
/// # Arguments
///
/// * `sql` - A SQL query string to translate.
/// * `schema` - Schema metadata with field capabilities.
///
/// # Returns
///
/// A [`Translation`] containing the RQL equivalent, or a [`SqlError`]
/// if the query is invalid or unsupported.
pub fn translate_with_schema(sql: &str, schema: &QuerySchema) -> Result<Translation, SqlError> {
    // Validate input
    validation::validate_query_string(sql)?;

    // Parse SQL
    let query = parser::parse(sql)?;

    // Validate parsed query
    validation::validate_query(&query)?;

    // Translate to RQL
    translator::translate_with_schema(query, schema)
}
