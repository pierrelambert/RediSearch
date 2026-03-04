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

#[cfg(test)]
mod tests {
    use super::*;

    // ErrorCategory Display tests
    #[test]
    fn test_error_category_display_syntax() {
        assert_eq!(format!("{}", ErrorCategory::Syntax), "Syntax error");
    }

    #[test]
    fn test_error_category_display_unsupported() {
        assert_eq!(
            format!("{}", ErrorCategory::Unsupported),
            "Unsupported feature"
        );
    }

    #[test]
    fn test_error_category_display_translation() {
        assert_eq!(
            format!("{}", ErrorCategory::Translation),
            "Translation error"
        );
    }

    // SqlError constructors tests
    #[test]
    fn test_sql_error_syntax() {
        let err = SqlError::syntax("Invalid syntax");
        assert_eq!(err.category, ErrorCategory::Syntax);
        assert_eq!(err.message, "Invalid syntax");
        assert!(err.position.is_none());
        assert!(err.suggestion.is_none());
    }

    #[test]
    fn test_sql_error_unsupported() {
        let err = SqlError::unsupported("Feature not supported");
        assert_eq!(err.category, ErrorCategory::Unsupported);
        assert_eq!(err.message, "Feature not supported");
    }

    #[test]
    fn test_sql_error_translation() {
        let err = SqlError::translation("Translation failed");
        assert_eq!(err.category, ErrorCategory::Translation);
        assert_eq!(err.message, "Translation failed");
    }

    // Builder pattern tests
    #[test]
    fn test_sql_error_with_position() {
        let err = SqlError::syntax("Error at position").with_position(42);
        assert_eq!(err.position, Some(42));
    }

    #[test]
    fn test_sql_error_with_suggestion() {
        let err = SqlError::syntax("Error").with_suggestion("Try this instead");
        assert_eq!(err.suggestion, Some("Try this instead".to_string()));
    }

    #[test]
    fn test_sql_error_with_position_and_suggestion() {
        let err = SqlError::syntax("Error")
            .with_position(10)
            .with_suggestion("Fix it");
        assert_eq!(err.position, Some(10));
        assert_eq!(err.suggestion, Some("Fix it".to_string()));
    }

    // Display/Error trait tests
    #[test]
    fn test_sql_error_display_syntax() {
        let err = SqlError::syntax("bad syntax");
        let display = format!("{err}");
        assert!(display.contains("Syntax error"));
        assert!(display.contains("bad syntax"));
    }

    #[test]
    fn test_sql_error_display_unsupported() {
        let err = SqlError::unsupported("JOINs not supported");
        let display = format!("{err}");
        assert!(display.contains("Unsupported feature"));
        assert!(display.contains("JOINs not supported"));
    }

    #[test]
    fn test_sql_error_display_translation() {
        let err = SqlError::translation("Failed to translate");
        let display = format!("{err}");
        assert!(display.contains("Translation error"));
        assert!(display.contains("Failed to translate"));
    }

    // From<ParserError> test
    #[test]
    fn test_sql_error_from_parser_error() {
        use sqlparser::dialect::GenericDialect;
        use sqlparser::parser::Parser;

        let dialect = GenericDialect {};
        let result = Parser::parse_sql(&dialect, "SELEC * FROM idx");
        assert!(result.is_err());

        let parser_err = result.unwrap_err();
        let sql_err: SqlError = parser_err.into();

        assert_eq!(sql_err.category, ErrorCategory::Syntax);
        assert!(!sql_err.message.is_empty());
    }

    // ErrorCategory equality tests
    #[test]
    fn test_error_category_equality() {
        assert_eq!(ErrorCategory::Syntax, ErrorCategory::Syntax);
        assert_eq!(ErrorCategory::Unsupported, ErrorCategory::Unsupported);
        assert_eq!(ErrorCategory::Translation, ErrorCategory::Translation);
        assert_ne!(ErrorCategory::Syntax, ErrorCategory::Unsupported);
        assert_ne!(ErrorCategory::Unsupported, ErrorCategory::Translation);
    }

    // ErrorCategory Clone/Copy tests
    #[test]
    fn test_error_category_clone() {
        let cat = ErrorCategory::Syntax;
        let cloned = cat;
        assert_eq!(cat, cloned);
    }

    // ErrorCategory Debug test
    #[test]
    fn test_error_category_debug() {
        let debug_str = format!("{:?}", ErrorCategory::Syntax);
        assert_eq!(debug_str, "Syntax");
    }

    // SqlError Debug test
    #[test]
    fn test_sql_error_debug() {
        let err = SqlError::syntax("test").with_position(5);
        let debug_str = format!("{err:?}");
        assert!(debug_str.contains("SqlError"));
        assert!(debug_str.contains("Syntax"));
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("5"));
    }

    // Test with String (not &str) to ensure Into<String> works
    #[test]
    fn test_sql_error_constructors_with_string() {
        let err1 = SqlError::syntax(String::from("error1"));
        let err2 = SqlError::unsupported(String::from("error2"));
        let err3 = SqlError::translation(String::from("error3"));

        assert_eq!(err1.message, "error1");
        assert_eq!(err2.message, "error2");
        assert_eq!(err3.message, "error3");
    }

    #[test]
    fn test_sql_error_suggestion_with_string() {
        let err = SqlError::syntax("Error").with_suggestion(String::from("suggestion"));
        assert_eq!(err.suggestion, Some("suggestion".to_string()));
    }
}
