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

use sql_parser::{
    CacheConfig, CacheStats, Command, FieldCapabilities, QuerySchema, clear_cache,
    get_cache_stats, set_cache_config, translate, translate_cached, translate_cached_with_schema,
};

/// Command type for the translation result.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlCommand {
    /// Use FT.SEARCH for this query.
    Search = 0,
    /// Use FT.AGGREGATE for this query.
    Aggregate = 1,
    /// Use FT.HYBRID for weighted vector + text search.
    Hybrid = 2,
}

impl From<Command> for SqlCommand {
    fn from(cmd: Command) -> Self {
        match cmd {
            Command::Search => SqlCommand::Search,
            Command::Aggregate => SqlCommand::Aggregate,
            Command::Hybrid => SqlCommand::Hybrid,
        }
    }
}

/// Schema capabilities for a single field supplied by the C caller.
#[repr(C)]
pub struct SqlSchemaField {
    /// Field name as a null-terminated UTF-8 string.
    pub name: *const c_char,
    /// Whether exact TAG-style matching is supported.
    pub supports_tag_queries: bool,
    /// Whether TEXT query semantics are supported.
    pub supports_text_queries: bool,
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

fn sql_translation_error(message: &str) -> SqlTranslationResult {
    SqlTranslationResult {
        success: false,
        command: SqlCommand::Search,
        index_name: ptr::null_mut(),
        query_string: ptr::null_mut(),
        arguments: ptr::null_mut(),
        arguments_len: 0,
        error_message: CString::new(message)
            .expect("CString::new failed for error")
            .into_raw(),
    }
}

fn sql_translation_from_result(
    result: Result<sql_parser::Translation, sql_parser::SqlError>,
) -> SqlTranslationResult {
    match result {
        Ok(translation) => {
            let index_name = CString::new(translation.index_name)
                .expect("CString::new failed for index_name")
                .into_raw();
            let query_string = CString::new(translation.query_string)
                .expect("CString::new failed for query_string")
                .into_raw();

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
        Err(err) => sql_translation_error(&err.to_string()),
    }
}

unsafe fn query_schema_from_ffi(
    version: u64,
    fields: *const SqlSchemaField,
    fields_len: usize,
) -> Result<QuerySchema, String> {
    let mut schema = QuerySchema::new(version);
    if fields_len == 0 {
        return Ok(schema);
    }
    if fields.is_null() {
        return Err("Schema fields pointer is null".to_string());
    }

    // SAFETY: The caller provides `fields` as a valid array of `fields_len` elements.
    let fields = unsafe { std::slice::from_raw_parts(fields, fields_len) };
    for field in fields {
        if field.name.is_null() {
            return Err("Schema field name is null".to_string());
        }

        // SAFETY: The caller provides `name` as a valid null-terminated UTF-8 string.
        let field_name = unsafe { CStr::from_ptr(field.name) }
            .to_str()
            .map_err(|_| "Schema field name contains invalid UTF-8".to_string())?;

        schema = schema.with_field(
            field_name,
            FieldCapabilities {
                supports_tag_queries: field.supports_tag_queries,
                supports_text_queries: field.supports_text_queries,
            },
        );
    }

    Ok(schema)
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
        return sql_translation_error("SQL query is null");
    }

    // SAFETY: Caller guarantees sql is a valid null-terminated string
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => return sql_translation_error("SQL query contains invalid UTF-8"),
    };

    sql_translation_from_result(translate(sql_str))
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

/// Translates a SQL query to RQL format with caching.
///
/// This function caches translation results, so repeated calls with the same
/// SQL string will return cached results (improving performance).
///
/// # Safety
///
/// - `sql` must be a valid null-terminated C string
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached(sql: *const c_char) -> SqlTranslationResult {
    if sql.is_null() {
        return sql_translation_error("SQL query is null");
    }

    // SAFETY: Caller guarantees sql is a valid null-terminated string
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => return sql_translation_error("SQL query contains invalid UTF-8"),
    };

    sql_translation_from_result(translate_cached(sql_str))
}

/// Translates a SQL query to RQL format with schema-aware caching.
///
/// The cache key includes the provided schema version, and the supplied field
/// capabilities are used to validate exact string matching semantics.
///
/// # Safety
///
/// - `sql` must be a valid null-terminated C string.
/// - If `fields_len > 0`, then `fields` must point to `fields_len` valid elements.
/// - Each `SqlSchemaField.name` must be a valid null-terminated UTF-8 string.
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached_with_schema(
    sql: *const c_char,
    schema_version: u64,
    fields: *const SqlSchemaField,
    fields_len: usize,
) -> SqlTranslationResult {
    if sql.is_null() {
        return sql_translation_error("SQL query is null");
    }

    // SAFETY: Caller guarantees sql is a valid null-terminated string.
    let sql_str = match unsafe { CStr::from_ptr(sql) }.to_str() {
        Ok(s) => s,
        Err(_) => return sql_translation_error("SQL query contains invalid UTF-8"),
    };

    let schema = match unsafe { query_schema_from_ffi(schema_version, fields, fields_len) } {
        Ok(schema) => schema,
        Err(message) => return sql_translation_error(&message),
    };

    sql_translation_from_result(translate_cached_with_schema(sql_str, &schema))
}

/// Cache statistics returned by `sql_cache_get_stats`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SqlCacheStats {
    /// Number of entries currently in the cache.
    pub entries: usize,
    /// Total number of cache hits.
    pub hits: u64,
    /// Total number of cache misses.
    pub misses: u64,
    /// Hit rate as an integer percentage (0-100).
    pub hit_rate_percent: u32,
}

impl From<CacheStats> for SqlCacheStats {
    fn from(stats: CacheStats) -> Self {
        Self {
            entries: stats.entries,
            hits: stats.hits,
            misses: stats.misses,
            hit_rate_percent: stats.hit_rate() as u32,
        }
    }
}

/// Clear the SQL translation cache.
///
/// This removes all cached translations and resets hit/miss statistics.
#[unsafe(no_mangle)]
pub extern "C" fn sql_cache_clear() {
    clear_cache();
}

/// Get SQL cache statistics.
///
/// Returns current cache state including entry count, hits, misses, and hit rate.
#[unsafe(no_mangle)]
pub extern "C" fn sql_cache_get_stats() -> SqlCacheStats {
    get_cache_stats().into()
}

/// Set the maximum number of entries in the SQL translation cache.
///
/// If the new limit is smaller than the current number of entries,
/// the least recently used entries are evicted.
#[unsafe(no_mangle)]
pub extern "C" fn sql_cache_set_max_entries(max_entries: usize) {
    set_cache_config(CacheConfig { max_entries });
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

    // Use a mutex to serialize cache-related tests
    use std::sync::Mutex;
    static CACHE_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn with_clean_cache<T>(f: impl FnOnce() -> T) -> T {
        let _guard = CACHE_TEST_MUTEX.lock().expect("Cache test mutex poisoned");
        sql_cache_clear();
        sql_cache_set_max_entries(1000); // Reset to default
        f()
    }

    #[test]
    fn test_ffi_cached_translation() {
        with_clean_cache(|| {
            let sql = CString::new("SELECT * FROM ffi_cached_idx").unwrap();

            // First call - should be a miss
            // SAFETY: sql is a valid null-terminated C string
            let result1 = unsafe { sql_translate_cached(sql.as_ptr()) };
            assert!(result1.success);
            let stats1 = sql_cache_get_stats();
            assert_eq!(stats1.misses, 1);

            // SAFETY: result1 was returned by sql_translate_cached
            unsafe {
                sql_translation_result_free(result1);
            }

            // Second call - should be a hit
            // SAFETY: sql is a valid null-terminated C string
            let result2 = unsafe { sql_translate_cached(sql.as_ptr()) };
            assert!(result2.success);
            let stats2 = sql_cache_get_stats();
            assert_eq!(stats2.hits, 1);

            // SAFETY: result2 was returned by sql_translate_cached
            unsafe {
                sql_translation_result_free(result2);
            }
        });
    }

    #[test]
    fn test_ffi_cached_translation_with_schema_rejects_text_equality() {
        with_clean_cache(|| {
            let sql = CString::new("SELECT * FROM idx WHERE title = 'redis'").unwrap();
            let field_name = CString::new("title").unwrap();
            let fields = [SqlSchemaField {
                name: field_name.as_ptr(),
                supports_tag_queries: false,
                supports_text_queries: true,
            }];

            let result = unsafe {
                sql_translate_cached_with_schema(sql.as_ptr(), 3, fields.as_ptr(), fields.len())
            };

            assert!(!result.success);
            unsafe {
                let error = CStr::from_ptr(result.error_message).to_str().unwrap();
                assert!(error.contains("TEXT field"));
                assert!(error.contains("MATCH"));
                sql_translation_result_free(result);
            }
        });
    }

    #[test]
    fn test_ffi_cache_stats() {
        with_clean_cache(|| {
            let stats = sql_cache_get_stats();
            assert_eq!(stats.entries, 0);
            assert_eq!(stats.hits, 0);
            assert_eq!(stats.misses, 0);
        });
    }

    #[test]
    fn test_ffi_cache_set_max_entries() {
        with_clean_cache(|| {
            sql_cache_set_max_entries(2);

            // Fill cache with 3 queries
            let sql1 = CString::new("SELECT * FROM ffi_evict_idx1").unwrap();
            let sql2 = CString::new("SELECT * FROM ffi_evict_idx2").unwrap();
            let sql3 = CString::new("SELECT * FROM ffi_evict_idx3").unwrap();

            // SAFETY: all sql strings are valid null-terminated C strings
            unsafe {
                let r1 = sql_translate_cached(sql1.as_ptr());
                sql_translation_result_free(r1);
                let r2 = sql_translate_cached(sql2.as_ptr());
                sql_translation_result_free(r2);
                let r3 = sql_translate_cached(sql3.as_ptr());
                sql_translation_result_free(r3);
            }

            // Should only have 2 entries due to eviction
            let stats = sql_cache_get_stats();
            assert_eq!(stats.entries, 2);
        });
    }

    #[test]
    fn test_ffi_vector_search() {
        // Test vector search with <-> operator (pgvector syntax)
        let sql =
            CString::new("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 5")
                .unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Search);

        // SAFETY: result was returned by sql_translate
        unsafe {
            let query_string = CStr::from_ptr(result.query_string).to_str().unwrap();
            assert_eq!(query_string, "*=>[KNN 5 @embedding $BLOB]");

            // Check arguments include PARAMS with vector blob
            let args: Vec<String> = (0..result.arguments_len)
                .map(|i| {
                    CStr::from_ptr(*result.arguments.add(i))
                        .to_str()
                        .unwrap()
                        .to_string()
                })
                .collect();
            assert!(args.contains(&"PARAMS".to_string()));
            assert!(args.contains(&"BLOB".to_string()));
            assert!(args.contains(&"[0.1, 0.2]".to_string()));

            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_vector_search_with_filter() {
        // Test hybrid search: filter + vector
        let sql = CString::new(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.5]' LIMIT 3",
        )
        .unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);

        // SAFETY: result was returned by sql_translate
        unsafe {
            let query_string = CStr::from_ptr(result.query_string).to_str().unwrap();
            assert_eq!(
                query_string,
                "@category:{electronics}=>[KNN 3 @embedding $BLOB]"
            );

            sql_translation_result_free(result);
        }
    }

    #[test]
    fn test_ffi_hybrid_search() {
        let sql = CString::new(
            "SELECT * FROM products \
             WHERE category = 'electronics' \
             ORDER BY embedding <-> '[0.5]' LIMIT 3 \
             OPTION (vector_weight = 0.8, text_weight = 0.2)",
        )
        .unwrap();
        // SAFETY: sql is a valid null-terminated C string
        let result = unsafe { sql_translate(sql.as_ptr()) };
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Hybrid);

        // SAFETY: result was returned by sql_translate
        unsafe {
            let query_string = CStr::from_ptr(result.query_string).to_str().unwrap();
            assert_eq!(query_string, "@category:{electronics}");

            let args: Vec<String> = (0..result.arguments_len)
                .map(|i| {
                    CStr::from_ptr(*result.arguments.add(i))
                        .to_str()
                        .unwrap()
                        .to_string()
                })
                .collect();
            assert_eq!(
                args,
                vec![
                    "VSIM",
                    "@embedding",
                    "$BLOB",
                    "KNN",
                    "2",
                    "K",
                    "3",
                    "COMBINE",
                    "LINEAR",
                    "4",
                    "ALPHA",
                    "0.8",
                    "BETA",
                    "0.2",
                    "LIMIT",
                    "0",
                    "3",
                    "PARAMS",
                    "2",
                    "BLOB",
                    "[0.5]",
                ]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
            );

            sql_translation_result_free(result);
        }
    }
}
