/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv3); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! FFI bindings for the query cache.

#![allow(non_camel_case_types, non_snake_case)]

use query_cache::{CacheKey, CacheStats, CachedResult, QueryCache, opaque::OpaqueQueryCache};

/// Create a new query cache with the specified maximum number of entries.
///
/// Returns a pointer to the cache that must be freed with `QueryCache_Free`.
///
/// # Safety
///
/// The returned pointer must be freed exactly once using `QueryCache_Free`.
#[unsafe(no_mangle)]
pub extern "C" fn QueryCache_New(max_entries: usize) -> *mut OpaqueQueryCache {
    let cache = Box::new(QueryCache::new(max_entries));
    cache.into_opaque()
}

/// Free a query cache.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
/// - `cache` must not be used after this call
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_Free(cache: *mut OpaqueQueryCache) {
    if cache.is_null() {
        return;
    }
    // SAFETY: Caller guarantees cache was created by QueryCache_New and is still valid
    let _cache = unsafe { QueryCache::from_opaque(cache) };
    // Cache is dropped here
}

/// Get a cached result.
///
/// Returns a pointer to the cached data and sets `size_out` to the data size.
/// Returns null if the key is not in the cache.
///
/// The returned pointer is valid until the cache is modified or freed.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
/// - `size_out` must be a valid pointer
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_Get(
    cache: *mut OpaqueQueryCache,
    query_hash: u64,
    index_revision: u64,
    size_out: *mut usize,
) -> *const u8 {
    if cache.is_null() || size_out.is_null() {
        return std::ptr::null();
    }

    let key = CacheKey {
        query_hash,
        index_revision,
    };

    // SAFETY: Caller guarantees cache is valid
    let cache_mut = unsafe { QueryCache::from_opaque_mut(cache) };

    if let Some(result) = cache_mut.get(&key) {
        // SAFETY: Caller guarantees size_out is valid
        unsafe {
            *size_out = result.data.len();
        }
        result.data.as_ptr()
    } else {
        std::ptr::null()
    }
}

/// Insert a result into the cache.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
/// - `data` must be a valid pointer to `data_size` bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_Insert(
    cache: *mut OpaqueQueryCache,
    query_hash: u64,
    index_revision: u64,
    data: *const u8,
    data_size: usize,
) {
    if cache.is_null() || data.is_null() {
        return;
    }

    let key = CacheKey {
        query_hash,
        index_revision,
    };

    // SAFETY: Caller guarantees data points to data_size valid bytes
    let data_vec = unsafe { std::slice::from_raw_parts(data, data_size) }.to_vec();

    let result = CachedResult {
        size_bytes: data_size,
        data: data_vec,
    };

    // SAFETY: Caller guarantees cache is valid
    let cache_mut = unsafe { QueryCache::from_opaque_mut(cache) };
    cache_mut.insert(key, result);
}

/// Clear all entries from the cache.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_Clear(cache: *mut OpaqueQueryCache) {
    if cache.is_null() {
        return;
    }

    // SAFETY: Caller guarantees cache is valid
    let cache_mut = unsafe { QueryCache::from_opaque_mut(cache) };
    cache_mut.clear();
}

/// Resize the cache to a new maximum number of entries.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_Resize(cache: *mut OpaqueQueryCache, new_max_entries: usize) {
    if cache.is_null() {
        return;
    }

    // SAFETY: Caller guarantees cache is valid
    let cache_mut = unsafe { QueryCache::from_opaque_mut(cache) };
    cache_mut.resize(new_max_entries);
}


/// C-compatible cache statistics structure.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QueryCacheStats {
    pub lookups: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
    pub memory_bytes: usize,
    pub hit_rate: f64,
}

impl From<CacheStats> for QueryCacheStats {
    fn from(stats: CacheStats) -> Self {
        Self {
            lookups: stats.lookups,
            hits: stats.hits,
            misses: stats.misses,
            evictions: stats.evictions,
            entries: stats.entries,
            memory_bytes: stats.memory_bytes,
            hit_rate: stats.hit_rate(),
        }
    }
}

/// Get cache statistics.
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_GetStats(cache: *const OpaqueQueryCache) -> QueryCacheStats {
    if cache.is_null() {
        return QueryCacheStats {
            lookups: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            entries: 0,
            memory_bytes: 0,
            hit_rate: 0.0,
        };
    }

    // SAFETY: Caller guarantees cache is valid
    let cache_ref = unsafe { QueryCache::from_opaque_ref(cache) };
    cache_ref.stats().into()
}

/// Reset statistics counters (but keep cached entries).
///
/// # Safety
///
/// - `cache` must have been created by `QueryCache_New`
/// - `cache` must not be null
#[unsafe(no_mangle)]
pub unsafe extern "C" fn QueryCache_ResetStats(cache: *mut OpaqueQueryCache) {
    if cache.is_null() {
        return;
    }

    // SAFETY: Caller guarantees cache is valid
    let cache_mut = unsafe { QueryCache::from_opaque_mut(cache) };
    cache_mut.reset_stats();
}

