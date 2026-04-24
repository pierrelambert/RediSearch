/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

mod aggregates;
mod expressions;
mod options;
mod preprocessor;
mod select;

use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::ast::SelectQuery;
use crate::error::SqlError;

use self::options::apply_hybrid_options;
use self::preprocessor::{extract_option_clause, quote_nonstandard_from_identifier};
use self::select::parse_statement;

/// Parsed OPTION clause values.
#[derive(Debug, Clone, Default)]
struct QueryOptions {
    /// Weight for vector scoring (0.0 to 1.0).
    vector_weight: Option<f64>,
    /// Weight for text scoring (0.0 to 1.0).
    text_weight: Option<f64>,
}

/// Parses a SQL query string into our internal AST representation.
pub fn parse(sql: &str) -> Result<SelectQuery, SqlError> {
    // Extract OPTION clause if present (non-standard SQL extension)
    let (sql_without_options, options) = extract_option_clause(sql)?;
    let sql_for_parser = quote_nonstandard_from_identifier(&sql_without_options);

    // Use PostgreSQL dialect to support <-> (L2 distance) operator from pgvector
    let dialect = PostgreSqlDialect {};
    let statements = Parser::parse_sql(&dialect, &sql_for_parser)?;

    if statements.is_empty() {
        return Err(SqlError::syntax("Empty query"));
    }

    if statements.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple statements are not supported",
        ));
    }

    let statement = statements.into_iter().next().unwrap();
    let mut query = parse_statement(statement)?;

    // Apply OPTION clause to create HybridSearch if weights specified with vector search
    apply_hybrid_options(&mut query, &options)?;

    Ok(query)
}
