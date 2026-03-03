/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! FFI bindings for the datetime parser.
//!
//! This crate provides C-callable functions for parsing ISO-8601 datetime strings.

use libc::{c_char, c_int};
use std::ffi::CStr;

/// Parse an ISO-8601 formatted string into a Unix timestamp.
///
/// # Arguments
///
/// * `str` - A null-terminated C string containing an ISO-8601 date or datetime
/// * `out_timestamp` - Pointer to store the resulting Unix timestamp (seconds since epoch)
///
/// # Returns
///
/// * `0` on success, with the timestamp written to `out_timestamp`
/// * `-1` on error (invalid format or null pointer)
///
/// # Safety
///
/// The following invariants must be upheld when calling this function:
/// - `str` must be a valid null-terminated C string or NULL
/// - `out_timestamp` must be a valid pointer to an `i64` or NULL
/// - If `str` or `out_timestamp` is NULL, the function returns -1
///
/// # Examples
///
/// ```c
/// int64_t timestamp;
/// int result = DateTime_ParseISO8601("2024-03-15T10:30:00Z", &timestamp);
/// if (result == 0) {
///     printf("Timestamp: %lld\n", timestamp);
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn DateTime_ParseISO8601(str: *const c_char, out_timestamp: *mut i64) -> c_int {
    // Check for null pointers
    if str.is_null() || out_timestamp.is_null() {
        return -1;
    }

    // SAFETY: The caller guarantees that `str` is a valid null-terminated C string.
    // We've already checked that it's not null above.
    let c_str = unsafe { CStr::from_ptr(str) };

    // Convert to Rust string
    let rust_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return -1, // Invalid UTF-8
    };

    // Parse the datetime
    match datetime::parse_iso8601(rust_str) {
        Ok(timestamp) => {
            // SAFETY: The caller guarantees that `out_timestamp` is a valid pointer.
            // We've already checked that it's not null above.
            unsafe {
                *out_timestamp = timestamp;
            }
            0
        }
        Err(_) => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn test_parse_iso8601_success() {
        let input = CString::new("2024-01-01T00:00:00Z").unwrap();
        let mut timestamp: i64 = 0;

        let result = unsafe { DateTime_ParseISO8601(input.as_ptr(), &mut timestamp) };

        assert_eq!(result, 0);
        assert_eq!(timestamp, 1704067200);
    }

    #[test]
    fn test_parse_iso8601_date_only() {
        let input = CString::new("2024-03-15").unwrap();
        let mut timestamp: i64 = 0;

        let result = unsafe { DateTime_ParseISO8601(input.as_ptr(), &mut timestamp) };

        assert_eq!(result, 0);
        assert_eq!(timestamp, 1710460800);
    }

    #[test]
    fn test_parse_iso8601_invalid() {
        let input = CString::new("not a date").unwrap();
        let mut timestamp: i64 = 0;

        let result = unsafe { DateTime_ParseISO8601(input.as_ptr(), &mut timestamp) };

        assert_eq!(result, -1);
    }

    #[test]
    fn test_parse_iso8601_null_str() {
        let mut timestamp: i64 = 0;

        let result = unsafe { DateTime_ParseISO8601(std::ptr::null(), &mut timestamp) };

        assert_eq!(result, -1);
    }

    #[test]
    fn test_parse_iso8601_null_output() {
        let input = CString::new("2024-01-01").unwrap();

        let result = unsafe { DateTime_ParseISO8601(input.as_ptr(), std::ptr::null_mut()) };

        assert_eq!(result, -1);
    }
}
