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
use std::panic::{AssertUnwindSafe, catch_unwind};
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
    let error_message = match CString::new(message) {
        Ok(error_message) => error_message,
        Err(_) => {
            let sanitized_message = message.replace('\0', "\\0");
            match CString::new(sanitized_message) {
                Ok(error_message) => error_message,
                Err(_) => literal_cstring("internal error: invalid error message"),
            }
        }
    };

    SqlTranslationResult {
        success: false,
        command: SqlCommand::Search,
        index_name: ptr::null_mut(),
        query_string: ptr::null_mut(),
        arguments: ptr::null_mut(),
        arguments_len: 0,
        error_message: error_message.into_raw(),
    }
}

fn literal_cstring(value: &str) -> CString {
    match CString::new(value) {
        Ok(value) => value,
        Err(_) => unreachable!("string literal unexpectedly contained NUL"),
    }
}

fn sql_translation_from_result(
    result: Result<sql_parser::Translation, sql_parser::SqlError>,
) -> SqlTranslationResult {
    match result {
        Ok(translation) => {
            let index_name = match CString::new(translation.index_name) {
                Ok(index_name) => index_name,
                Err(_) => {
                    return sql_translation_error("translated index name contains embedded NUL byte");
                }
            };
            let query_string = match CString::new(translation.query_string) {
                Ok(query_string) => query_string,
                Err(_) => {
                    return sql_translation_error("translated query string contains embedded NUL byte");
                }
            };

            let argument_cstrings = match translation
                .arguments
                .into_iter()
                .map(CString::new)
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(arguments) => arguments,
                Err(_) => return sql_translation_error("translated argument contains embedded NUL byte"),
            };

            let arguments_len = argument_cstrings.len();
            let arguments = if arguments_len > 0 {
                let mut args: Vec<*mut c_char> = argument_cstrings
                    .into_iter()
                    .map(CString::into_raw)
                    .collect();
                args.shrink_to_fit();
                debug_assert_eq!(args.len(), args.capacity());
                let ptr = args.as_mut_ptr();
                std::mem::forget(args);
                ptr
            } else {
                ptr::null_mut()
            };

            SqlTranslationResult {
                success: true,
                command: translation.command.into(),
                index_name: index_name.into_raw(),
                query_string: query_string.into_raw(),
                arguments,
                arguments_len,
                error_message: ptr::null_mut(),
            }
        }
        Err(err) => sql_translation_error(&err.to_string()),
    }
}

unsafe fn sql_input_from_ffi<'a>(sql: *const u8, sql_len: usize) -> Result<&'a str, SqlTranslationResult> {
    if sql.is_null() {
        return Err(sql_translation_error("SQL query is null"));
    }

    if sql_len == 0 {
        return Ok("");
    }

    // SAFETY: Caller guarantees `sql` points to `sql_len` bytes for the duration of this call.
    let sql_bytes = unsafe { std::slice::from_raw_parts(sql, sql_len) };
    if sql_bytes.contains(&0) {
        return Err(sql_translation_error("SQL query contains embedded NUL byte"));
    }

    std::str::from_utf8(sql_bytes)
        .map_err(|_| sql_translation_error("SQL query contains invalid UTF-8"))
}

fn translate_entrypoint(
    sql: *const u8,
    sql_len: usize,
    translator: impl FnOnce(&str) -> Result<sql_parser::Translation, sql_parser::SqlError>,
) -> SqlTranslationResult {
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `translate_entrypoint` preserves the FFI contract for `sql` and `sql_len`.
        let sql = match unsafe { sql_input_from_ffi(sql, sql_len) } {
            Ok(sql) => sql,
            Err(err) => return err,
        };

        sql_translation_from_result(translator(sql))
    })) {
        Ok(result) => result,
        Err(_) => sql_translation_error("internal error: unexpected panic"),
    }
}

fn translate_with_schema_entrypoint(
    sql: *const u8,
    sql_len: usize,
    schema_version: u64,
    fields: *const SqlSchemaField,
    fields_len: usize,
) -> SqlTranslationResult {
    match catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `translate_with_schema_entrypoint` preserves the FFI contract for `sql` and `sql_len`.
        let sql = match unsafe { sql_input_from_ffi(sql, sql_len) } {
            Ok(sql) => sql,
            Err(err) => return err,
        };

        // SAFETY: `translate_with_schema_entrypoint` preserves the FFI contract for `fields`.
        let schema = match unsafe { query_schema_from_ffi(schema_version, fields, fields_len) } {
            Ok(schema) => schema,
            Err(message) => return sql_translation_error(&message),
        };

        sql_translation_from_result(translate_cached_with_schema(sql, &schema))
    })) {
        Ok(result) => result,
        Err(_) => sql_translation_error("internal error: unexpected panic"),
    }
}

fn catch_unwind_or_default<T: Default>(f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_default()
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
/// - `sql` must point to `sql_len` readable bytes for the duration of this call.
/// - `sql` must contain valid UTF-8 and no embedded NUL bytes.
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate(sql: *const u8, sql_len: usize) -> SqlTranslationResult {
    translate_entrypoint(sql, sql_len, translate)
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
    catch_unwind_or_default(|| {
        if !result.index_name.is_null() {
            // SAFETY: index_name was created by CString::into_raw in sql_translate.
            unsafe {
                drop(CString::from_raw(result.index_name));
            }
        }
        if !result.query_string.is_null() {
            // SAFETY: query_string was created by CString::into_raw in sql_translate.
            unsafe {
                drop(CString::from_raw(result.query_string));
            }
        }
        if !result.error_message.is_null() {
            // SAFETY: error_message was created by CString::into_raw in sql_translate.
            unsafe {
                drop(CString::from_raw(result.error_message));
            }
        }

        if !result.arguments.is_null() && result.arguments_len > 0 {
            // SAFETY: arguments was created from a Vec leaked with len == capacity.
            let args = unsafe {
                Vec::from_raw_parts(result.arguments, result.arguments_len, result.arguments_len)
            };
            for arg in args {
                if !arg.is_null() {
                    // SAFETY: Each arg was created by CString::into_raw.
                    unsafe {
                        drop(CString::from_raw(arg));
                    }
                }
            }
        }
    });
}

/// Translates a SQL query to RQL format with caching.
///
/// This function caches translation results, so repeated calls with the same
/// SQL string will return cached results (improving performance).
///
/// # Safety
///
/// - `sql` must point to `sql_len` readable bytes for the duration of this call.
/// - `sql` must contain valid UTF-8 and no embedded NUL bytes.
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached(
    sql: *const u8,
    sql_len: usize,
) -> SqlTranslationResult {
    translate_entrypoint(sql, sql_len, translate_cached)
}

/// Translates a SQL query to RQL format with schema-aware caching.
///
/// The cache key includes the provided schema version, and the supplied field
/// capabilities are used to validate exact string matching semantics.
///
/// # Safety
///
/// - `sql` must point to `sql_len` readable bytes for the duration of this call.
/// - `sql` must contain valid UTF-8 and no embedded NUL bytes.
/// - If `fields_len > 0`, then `fields` must point to `fields_len` valid elements.
/// - Each `SqlSchemaField.name` must be a valid null-terminated UTF-8 string.
/// - The returned `SqlTranslationResult` must be freed with `sql_translation_result_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached_with_schema(
    sql: *const u8,
    sql_len: usize,
    schema_version: u64,
    fields: *const SqlSchemaField,
    fields_len: usize,
) -> SqlTranslationResult {
    translate_with_schema_entrypoint(sql, sql_len, schema_version, fields, fields_len)
}

/// Cache statistics returned by `sql_cache_get_stats`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
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
    catch_unwind_or_default(clear_cache);
}

/// Get SQL cache statistics.
///
/// Returns current cache state including entry count, hits, misses, and hit rate.
#[unsafe(no_mangle)]
pub extern "C" fn sql_cache_get_stats() -> SqlCacheStats {
    catch_unwind_or_default(|| get_cache_stats().into())
}

/// Set the maximum number of entries in the SQL translation cache.
///
/// If the new limit is smaller than the current number of entries,
/// the least recently used entries are evicted.
#[unsafe(no_mangle)]
pub extern "C" fn sql_cache_set_max_entries(max_entries: usize) {
    catch_unwind_or_default(|| set_cache_config(CacheConfig { max_entries }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CACHE_TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn cstring(value: &str) -> CString {
        CString::new(value)
            .unwrap_or_else(|err| panic!("test string unexpectedly contained NUL: {err}"))
    }

    fn call_sql_translate(sql: &CString) -> SqlTranslationResult {
        // SAFETY: `sql` points to a valid byte slice for the duration of the call.
        unsafe { sql_translate(sql.as_ptr().cast(), sql.as_bytes().len()) }
    }

    fn call_sql_translate_cached(sql: &CString) -> SqlTranslationResult {
        // SAFETY: `sql` points to a valid byte slice for the duration of the call.
        unsafe { sql_translate_cached(sql.as_ptr().cast(), sql.as_bytes().len()) }
    }

    fn call_sql_translate_cached_with_schema(
        sql: &CString,
        schema_version: u64,
        fields: *const SqlSchemaField,
        fields_len: usize,
    ) -> SqlTranslationResult {
        // SAFETY: `sql` points to a valid byte slice and `fields` follows the FFI contract.
        unsafe {
            sql_translate_cached_with_schema(
                sql.as_ptr().cast(),
                sql.as_bytes().len(),
                schema_version,
                fields,
                fields_len,
            )
        }
    }

    fn call_raw_sql_translate(sql: &[u8]) -> SqlTranslationResult {
        // SAFETY: `sql` points to `sql.len()` readable bytes for the duration of the call.
        unsafe { sql_translate(sql.as_ptr(), sql.len()) }
    }

    fn c_str_to_string(value: *const c_char) -> String {
        // SAFETY: The caller guarantees `value` points to a valid null-terminated string.
        unsafe { CStr::from_ptr(value) }
            .to_str()
            .unwrap_or_else(|err| panic!("invalid UTF-8 in FFI output: {err}"))
            .to_owned()
    }

    fn argument_strings(result: &SqlTranslationResult) -> Vec<String> {
        (0..result.arguments_len)
            .map(|i| {
                // SAFETY: `arguments` points to `arguments_len` valid C string pointers.
                let argument = unsafe { std::ptr::read(result.arguments.wrapping_add(i)) };
                c_str_to_string(argument)
            })
            .collect()
    }

    fn free_result(result: SqlTranslationResult) {
        // SAFETY: `result` was returned by one of the sql_parser_ffi translation functions.
        unsafe { sql_translation_result_free(result) }
    }

    fn with_clean_cache<T>(f: impl FnOnce() -> T) -> T {
        let _guard = CACHE_TEST_MUTEX
            .lock()
            .unwrap_or_else(|err| panic!("Cache test mutex poisoned: {err}"));
        sql_cache_clear();
        sql_cache_set_max_entries(1000);
        f()
    }

    #[test]
    fn test_ffi_simple_query() {
        let sql = cstring("SELECT * FROM idx");
        let result = call_sql_translate(&sql);
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Search);
        assert!(!result.index_name.is_null());
        assert!(!result.query_string.is_null());

        assert_eq!(c_str_to_string(result.index_name), "idx");
        assert_eq!(c_str_to_string(result.query_string), "*");
        free_result(result);
    }

    #[test]
    fn test_ffi_query_with_where() {
        let sql = cstring("SELECT * FROM products WHERE price > 100");
        let result = call_sql_translate(&sql);
        assert!(result.success);

        assert_eq!(c_str_to_string(result.index_name), "products");
        assert_eq!(c_str_to_string(result.query_string), "@price:[(100 +inf]");
        free_result(result);
    }

    #[test]
    fn test_ffi_query_with_arguments() {
        let sql = cstring("SELECT name, price FROM products LIMIT 10");
        let result = call_sql_translate(&sql);
        assert!(result.success);
        assert!(result.arguments_len > 0);
        assert!(!result.arguments.is_null());

        free_result(result);
    }

    #[test]
    fn test_ffi_error_handling() {
        let sql = cstring("INVALID SQL");
        let result = call_sql_translate(&sql);
        assert!(!result.success);
        assert!(!result.error_message.is_null());
        assert!(result.index_name.is_null());
        assert!(result.query_string.is_null());

        assert!(!c_str_to_string(result.error_message).is_empty());
        free_result(result);
    }

    #[test]
    fn test_ffi_null_input() {
        // SAFETY: Testing null pointer handling.
        let result = unsafe { sql_translate(std::ptr::null(), 4) };
        assert!(!result.success);
        assert!(!result.error_message.is_null());

        assert_eq!(c_str_to_string(result.error_message), "SQL query is null");
        free_result(result);
    }

    #[test]
    fn test_ffi_embedded_nul_input() {
        let sql = b"SELECT * FROM idx\0 WHERE price > 1";

        let result = call_raw_sql_translate(sql);
        assert!(!result.success);

        assert_eq!(
            c_str_to_string(result.error_message),
            "SQL query contains embedded NUL byte"
        );
        free_result(result);
    }

    #[test]
    fn test_ffi_invalid_utf8_input() {
        let sql = [0xff_u8, b'S', b'E', b'L', b'E', b'C', b'T'];

        let result = call_raw_sql_translate(&sql);
        assert!(!result.success);

        assert_eq!(
            c_str_to_string(result.error_message),
            "SQL query contains invalid UTF-8"
        );
        free_result(result);
    }

    #[test]
    fn test_ffi_translation_panic_is_caught() {
        let sql = b"SELECT * FROM idx";
        let result = translate_entrypoint(sql.as_ptr(), sql.len(), |_| panic!("boom"));
        assert!(!result.success);

        assert_eq!(
            c_str_to_string(result.error_message),
            "internal error: unexpected panic"
        );
        free_result(result);
    }

    #[test]
    fn test_ffi_cached_translation() {
        with_clean_cache(|| {
            let sql = cstring("SELECT * FROM ffi_cached_idx");

            let result1 = call_sql_translate_cached(&sql);
            assert!(result1.success);
            let stats1 = sql_cache_get_stats();
            assert_eq!(stats1.misses, 1);

            free_result(result1);

            let result2 = call_sql_translate_cached(&sql);
            assert!(result2.success);
            let stats2 = sql_cache_get_stats();
            assert_eq!(stats2.hits, 1);

            free_result(result2);
        });
    }

    #[test]
    fn test_ffi_cached_translation_with_schema_rejects_text_equality() {
        with_clean_cache(|| {
            let sql = cstring("SELECT * FROM idx WHERE title = 'redis'");
            let field_name = cstring("title");
            let fields = [SqlSchemaField {
                name: field_name.as_ptr(),
                supports_tag_queries: false,
                supports_text_queries: true,
            }];

            let result =
                call_sql_translate_cached_with_schema(&sql, 3, fields.as_ptr(), fields.len());

            assert!(!result.success);
            let error = c_str_to_string(result.error_message);
            assert!(error.contains("TEXT field"));
            assert!(error.contains("MATCH"));
            free_result(result);
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

            let sql1 = cstring("SELECT * FROM ffi_evict_idx1");
            let sql2 = cstring("SELECT * FROM ffi_evict_idx2");
            let sql3 = cstring("SELECT * FROM ffi_evict_idx3");

            let r1 = call_sql_translate_cached(&sql1);
            let r2 = call_sql_translate_cached(&sql2);
            let r3 = call_sql_translate_cached(&sql3);

            free_result(r1);
            free_result(r2);
            free_result(r3);

            let stats = sql_cache_get_stats();
            assert_eq!(stats.entries, 2);
        });
    }

    #[test]
    fn test_ffi_vector_search() {
        let sql = cstring("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 5");
        let result = call_sql_translate(&sql);
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Search);

        assert_eq!(c_str_to_string(result.query_string), "*=>[KNN 5 @embedding $BLOB]");
        let args = argument_strings(&result);
        assert!(args.contains(&"PARAMS".to_string()));
        assert!(args.contains(&"BLOB".to_string()));
        assert!(args.contains(&"[0.1, 0.2]".to_string()));
        free_result(result);
    }

    #[test]
    fn test_ffi_vector_search_with_filter() {
        let sql = cstring(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.5]' LIMIT 3",
        );
        let result = call_sql_translate(&sql);
        assert!(result.success);

        assert_eq!(
            c_str_to_string(result.query_string),
            "@category:{electronics}=>[KNN 3 @embedding $BLOB]"
        );
        free_result(result);
    }

    #[test]
    fn test_ffi_hybrid_search() {
        let sql = cstring(
            "SELECT * FROM products \
             WHERE category = 'electronics' \
             ORDER BY embedding <-> '[0.5]' LIMIT 3 \
             OPTION (vector_weight = 0.8, text_weight = 0.2)",
        );
        let result = call_sql_translate(&sql);
        assert!(result.success);
        assert_eq!(result.command, SqlCommand::Hybrid);

        assert_eq!(c_str_to_string(result.query_string), "@category:{electronics}");
        assert_eq!(
            argument_strings(&result),
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
        free_result(result);
    }
}
