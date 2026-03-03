/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! FFI bindings for the SQL parser.
//!
//! This module provides C-callable functions for translating SQL queries
//! to RediSearch Query Language (RQL).

#![allow(non_camel_case_types)]

use std::ffi::{CStr, CString, c_char};
use std::ptr;

use sql_parser::{Command, translate};

/// Command type for the translation result.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlCommand {
    /// Use FT.SEARCH for this query.
    Search = 0,
    /// Use FT.AGGREGATE for this query.
    Aggregate = 1,
}

impl From<Command> for SqlCommand {
    fn from(cmd: Command) -> Self {
        match cmd {
            Command::Search => SqlCommand::Search,
            Command::Aggregate => SqlCommand::Aggregate,
        }
    }
}

/// Result structure for SQL to RQL translation.
///
/// On success:
/// - `success` is true
/// - `command` indicates whether to use FT.SEARCH or FT.AGGREGATE
/// - `index_name` contains the index name (must be freed)
/// - `query_string` contains the RQL query (must be freed)
/// - `arguments` contains additional arguments (must be freed)
/// - `arguments_len` is the number of arguments
/// - `error_message` is null
///
/// On failure:
/// - `success` is false
/// - `error_message` contains the error description (must be freed)
/// - Other fields are zeroed/null
#[repr(C)]
pub struct SqlTranslationResult {
    /// Whether the translation succeeded.
    pub success: bool,
    /// The command type (0 = Search, 1 = Aggregate).
    pub command: SqlCommand,
    /// The index name from the FROM clause. Null on error.
    pub index_name: *mut c_char,
    /// The RQL query string. Null on error.
    pub query_string: *mut c_char,
    /// Array of additional arguments (RETURN, SORTBY, LIMIT, etc.). Null on error.
    pub arguments: *mut *mut c_char,
    /// Number of elements in the arguments array.
    pub arguments_len: usize,
    /// Error message if translation failed. Null on success.
    pub error_message: *mut c_char,
}

/// Translates a SQL query to RQL format.
///
/// # Safety
///
/// - `sql` must be a valid null-terminated C string
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate(sql: *const c_char) -> SqlTranslationResult {
    if sql.is_null() {
        return SqlTranslationResult {
            success: false,
            command: SqlCommand::Search,
            index_name: ptr::null_mut(),
            query_string: ptr::null_mut(),
            arguments: ptr::null_mut(),
            arguments_len: 0,
            error_message: CString::new("SQL query is null")
                .expect("CString::new failed")
                .into_raw(),
        };
    }

    // SAFETY: Caller guarantees sql is a valid null-terminated string
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => {
            return SqlTranslationResult {
                success: false,
                command: SqlCommand::Search,
                index_name: ptr::null_mut(),
                query_string: ptr::null_mut(),
                arguments: ptr::null_mut(),
                arguments_len: 0,
                error_message: CString::new("SQL query contains invalid UTF-8")
                    .expect("CString::new failed")
                    .into_raw(),
            };
        }
    };

    match translate(sql_str) {
        Ok(translation) => {
            let index_name = CString::new(translation.index_name)
                .expect("CString::new failed for index_name")
                .into_raw();
            let query_string = CString::new(translation.query_string)
                .expect("CString::new failed for query_string")
                .into_raw();

            // Convert arguments Vec<String> to C array
            let arguments_len = translation.arguments.len();
            let arguments = if arguments_len > 0 {
                let mut args: Vec<*mut c_char> = translation
                    .arguments
                    .into_iter()
                    .map(|arg| {
                        CString::new(arg)
                            .expect("CString::new failed for argument")
                            .into_raw()
                    })
                    .collect();
                let ptr = args.as_mut_ptr();
                std::mem::forget(args);
                ptr
            } else {
                ptr::null_mut()
            };

            SqlTranslationResult {
                success: true,
                command: translation.command.into(),
                index_name,
                query_string,
                arguments,
                arguments_len,
                error_message: ptr::null_mut(),
            }
        }
        Err(err) => SqlTranslationResult {
            success: false,
            command: SqlCommand::Search,
            index_name: ptr::null_mut(),
            query_string: ptr::null_mut(),
            arguments: ptr::null_mut(),
            arguments_len: 0,
            error_message: CString::new(err.to_string())
                .expect("CString::new failed for error")
                .into_raw(),
        },
    }
}

/// Frees a `SqlTranslationResult` returned by `sql_translate`.
///
/// # Safety
///
/// - `result` must have been returned by `sql_translate`
/// - `result` must not be used after calling this function
/// - This function must be called exactly once per result
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translation_result_free(result: SqlTranslationResult) {
    if !result.index_name.is_null() {
        // SAFETY: index_name was created by CString::into_raw in sql_translate
        unsafe {
            drop(CString::from_raw(result.index_name));
        }
    }
    if !result.query_string.is_null() {
        // SAFETY: query_string was created by CString::into_raw in sql_translate
        unsafe {
            drop(CString::from_raw(result.query_string));
        }
    }
    if !result.error_message.is_null() {
        // SAFETY: error_message was created by CString::into_raw in sql_translate
        unsafe {
            drop(CString::from_raw(result.error_message));
        }
    }
    // Free the arguments array
    if !result.arguments.is_null() && result.arguments_len > 0 {
        // SAFETY: arguments was created from Vec::as_mut_ptr with mem::forget in sql_translate
        let args = unsafe {
            Vec::from_raw_parts(result.arguments, result.arguments_len, result.arguments_len)
        };
        for arg in args {
            if !arg.is_null() {
                // SAFETY: Each arg was created by CString::into_raw
                unsafe {
                    drop(CString::from_raw(arg));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ffi_simple_query() {
        let sql = CString::new("SELECT * FROM idx").unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Search);
        assert!(!result.index_name.is_null());
        assert!(!result.query_string.is_null());

        // SAFETY: result was returned by sql_translate
        unsafe {
            let index_name = CStr::from_ptr(result.index_name).to_str().unwrap();
            assert_eq!(index_name, "idx");

            let query_string = CStr::from_ptr(result.query_string).to_str().unwrap();
            assert_eq!(query_string, "*");

            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_query_with_where() {
        let sql = CString::new("SELECT * FROM products WHERE price > 100").unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);

        // SAFETY: result was returned by sql_translate
        unsafe {
            let index_name = CStr::from_ptr(result.index_name).to_str().unwrap();
            assert_eq!(index_name, "products");

            let query_string = CStr::from_ptr(result.query_string).to_str().unwrap();
            assert_eq!(query_string, "@price:[(100 +inf]");

            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_query_with_arguments() {
        let sql = CString::new("SELECT name, price FROM products LIMIT 10").unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);
        assert!(result.arguments_len > 0);
        assert!(!result.arguments.is_null());

        // SAFETY: result was returned by sql_translate
        unsafe {
            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_error_handling() {
        let sql = CString::new("INVALID SQL").unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(!result.success);
        assert!(!result.error_message.is_null());
        assert!(result.index_name.is_null());
        assert!(result.query_string.is_null());

        // SAFETY: result was returned by sql_translate
        unsafe {
            let error = CStr::from_ptr(result.error_message).to_str().unwrap();
            assert!(!error.is_empty());

            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_null_input() {
        // SAFETY: Testing null pointer handling
        let result = unsafe { sql_translate(ptr::null()) };
        assert!(!result.success);
        assert!(!result.error_message.is_null());

        // SAFETY: result was returned by sql_translate
        unsafe {
            sql_translation_result_free(result);
        }
    }
}
