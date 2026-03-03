/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Error types for SQL parsing and translation.

use thiserror::Error;

/// Error category for SQL translation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Invalid SQL syntax.
    Syntax,
    /// Unsupported SQL feature.
    Unsupported,
    /// Error during translation to RQL.
    Translation,
}

impl std::fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax => write!(f, "Syntax error"),
            Self::Unsupported => write!(f, "Unsupported feature"),
            Self::Translation => write!(f, "Translation error"),
        }
    }
}

/// Error returned by SQL parsing and translation operations.
#[derive(Debug, Error)]
#[error("SQL Error: {category}: {message}")]
pub struct SqlError {
    /// The category of the error.
    pub category: ErrorCategory,
    /// Human-readable error message.
    pub message: String,
    /// Character position in the SQL string where the error occurred (if known).
    pub position: Option<usize>,
    /// Suggestion for fixing the error (if available).
    pub suggestion: Option<String>,
}

impl SqlError {
    /// Creates a new syntax error.
    pub fn syntax(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Syntax,
            message: message.into(),
            position: None,
            suggestion: None,
        }
    }

    /// Creates a new unsupported feature error.
    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Unsupported,
            message: message.into(),
            position: None,
            suggestion: None,
        }
    }

    /// Creates a new translation error.
    pub fn translation(message: impl Into<String>) -> Self {
        Self {
            category: ErrorCategory::Translation,
            message: message.into(),
            position: None,
            suggestion: None,
        }
    }

    /// Adds position information to the error.
    #[must_use]
    pub const fn with_position(mut self, position: usize) -> Self {
        self.position = Some(position);
        self
    }

    /// Adds a suggestion to the error.
    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

impl From<sqlparser::parser::ParserError> for SqlError {
    fn from(err: sqlparser::parser::ParserError) -> Self {
        Self::syntax(err.to_string())
    }
}

