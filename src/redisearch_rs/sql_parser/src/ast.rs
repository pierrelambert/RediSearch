/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Internal AST representation for SQL queries.
//!
//! This module defines a simplified AST that captures only the SQL
//! constructs supported by the RediSearch query language.

/// A simplified SELECT query representation.
#[derive(Debug, Clone)]
pub struct SelectQuery {
    /// Fields to return. Empty means SELECT *.
    pub fields: Vec<String>,
    /// The index name (from FROM clause).
    pub index_name: String,
    /// WHERE clause conditions.
    pub conditions: Vec<Condition>,
    /// ORDER BY clause.
    pub order_by: Option<OrderBy>,
    /// LIMIT clause.
    pub limit: Option<Limit>,
}

/// A single condition in the WHERE clause.
#[derive(Debug, Clone)]
pub enum Condition {
    /// Equality: field = value
    Equals { field: String, value: Value },
    /// Greater than: field > value
    GreaterThan { field: String, value: Value },
    /// Greater than or equal: field >= value
    GreaterThanOrEqual { field: String, value: Value },
    /// Less than: field < value
    LessThan { field: String, value: Value },
    /// Less than or equal: field <= value
    LessThanOrEqual { field: String, value: Value },
    /// Between: field BETWEEN low AND high
    Between {
        field: String,
        low: Value,
        high: Value,
    },
}

impl Condition {
    /// Returns the field name this condition applies to.
    pub fn field(&self) -> &str {
        match self {
            Self::Equals { field, .. }
            | Self::GreaterThan { field, .. }
            | Self::GreaterThanOrEqual { field, .. }
            | Self::LessThan { field, .. }
            | Self::LessThanOrEqual { field, .. }
            | Self::Between { field, .. } => field,
        }
    }
}

/// A value in a SQL expression.
#[derive(Debug, Clone)]
pub enum Value {
    /// A string literal.
    String(String),
    /// A numeric literal (integer or float).
    Number(f64),
}

impl Value {
    /// Converts the value to a string representation suitable for RQL.
    pub fn to_rql_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Number(n) => {
                // Format integers without decimal point
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
        }
    }
}

/// ORDER BY clause.
#[derive(Debug, Clone)]
pub struct OrderBy {
    /// Field to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction for ORDER BY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

impl std::fmt::Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asc => write!(f, "ASC"),
            Self::Desc => write!(f, "DESC"),
        }
    }
}

/// LIMIT clause.
#[derive(Debug, Clone, Copy)]
pub struct Limit {
    /// Maximum number of results to return.
    pub count: u64,
    /// Number of results to skip (OFFSET).
    pub offset: u64,
}

