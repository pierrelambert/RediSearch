/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! ISO-8601 datetime parsing for RediSearch DATETIME fields.
//!
//! This crate provides functionality to parse ISO-8601 formatted date and datetime strings
//! into Unix timestamps (seconds since epoch).
//!
//! # Supported Formats
//!
//! - Date only: `2024-03-15` → midnight UTC
//! - DateTime with UTC: `2024-03-15T10:30:00Z`
//! - DateTime with timezone: `2024-03-15T10:30:00+05:00`
//! - DateTime with fractional seconds: `2024-03-15T10:30:00.123Z`
//!
//! # Examples
//!
//! ```
//! use datetime::parse_iso8601;
//!
//! // Parse date only (assumes midnight UTC)
//! assert_eq!(parse_iso8601("2024-01-01"), Ok(1704067200));
//!
//! // Parse datetime with UTC timezone
//! assert_eq!(parse_iso8601("2024-01-01T00:00:00Z"), Ok(1704067200));
//!
//! // Parse datetime with offset
//! assert_eq!(parse_iso8601("2024-01-01T05:00:00+05:00"), Ok(1704067200));
//! ```

use chrono::{DateTime, NaiveDate, TimeZone, Utc};

/// Error type for datetime parsing failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The input string is not a valid ISO-8601 date or datetime.
    InvalidFormat,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => write!(f, "Invalid ISO-8601 format"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse an ISO-8601 formatted string into a Unix timestamp (seconds since epoch).
///
/// # Arguments
///
/// * `s` - An ISO-8601 formatted date or datetime string
///
/// # Returns
///
/// * `Ok(i64)` - Unix timestamp in seconds
/// * `Err(ParseError)` - If the string is not a valid ISO-8601 format
///
/// # Examples
///
/// ```
/// use datetime::parse_iso8601;
///
/// // Date only (midnight UTC)
/// assert_eq!(parse_iso8601("2024-03-15"), Ok(1710460800));
///
/// // DateTime with UTC
/// assert_eq!(parse_iso8601("2024-03-15T10:30:00Z"), Ok(1710498600));
///
/// // DateTime with timezone offset
/// assert_eq!(parse_iso8601("2024-03-15T15:30:00+05:00"), Ok(1710498600));
/// ```
pub fn parse_iso8601(s: &str) -> Result<i64, ParseError> {
    // Try parsing as full datetime with timezone first
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.timestamp());
    }

    // Try parsing as date only (YYYY-MM-DD)
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        // Convert to midnight UTC
        let datetime = date.and_hms_opt(0, 0, 0).ok_or(ParseError::InvalidFormat)?;
        let utc_datetime = Utc.from_utc_datetime(&datetime);
        return Ok(utc_datetime.timestamp());
    }

    Err(ParseError::InvalidFormat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_date_only() {
        // 2024-01-01 00:00:00 UTC
        assert_eq!(parse_iso8601("2024-01-01"), Ok(1704067200));

        // 2024-03-15 00:00:00 UTC
        assert_eq!(parse_iso8601("2024-03-15"), Ok(1710460800));
    }

    #[test]
    fn test_parse_datetime_utc() {
        // 2024-01-01 00:00:00 UTC
        assert_eq!(parse_iso8601("2024-01-01T00:00:00Z"), Ok(1704067200));

        // 2024-03-15 10:30:00 UTC
        assert_eq!(parse_iso8601("2024-03-15T10:30:00Z"), Ok(1710498600));
    }

    #[test]
    fn test_parse_datetime_with_offset() {
        // 2024-01-01 05:00:00+05:00 = 2024-01-01 00:00:00 UTC
        assert_eq!(parse_iso8601("2024-01-01T05:00:00+05:00"), Ok(1704067200));

        // 2024-03-15 15:30:00+05:00 = 2024-03-15 10:30:00 UTC
        assert_eq!(parse_iso8601("2024-03-15T15:30:00+05:00"), Ok(1710498600));
    }

    #[test]
    fn test_parse_datetime_with_fractional_seconds() {
        // Fractional seconds should be ignored
        assert_eq!(parse_iso8601("2024-01-01T00:00:00.123Z"), Ok(1704067200));
        assert_eq!(parse_iso8601("2024-01-01T00:00:00.999999Z"), Ok(1704067200));
    }

    #[test]
    fn test_parse_invalid_format() {
        assert_eq!(parse_iso8601("not a date"), Err(ParseError::InvalidFormat));
        assert_eq!(parse_iso8601("2024-13-01"), Err(ParseError::InvalidFormat)); // Invalid month
        assert_eq!(parse_iso8601("2024-01-32"), Err(ParseError::InvalidFormat)); // Invalid day
        assert_eq!(parse_iso8601(""), Err(ParseError::InvalidFormat));
    }

    #[test]
    fn test_parse_various_formats() {
        // Different valid ISO-8601 formats
        assert!(parse_iso8601("2024-01-01T00:00:00+00:00").is_ok());
        assert!(parse_iso8601("2024-01-01T00:00:00-05:00").is_ok());
        assert!(parse_iso8601("2024-12-31T23:59:59Z").is_ok());
    }
}
