/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL translation cache for avoiding redundant parsing and translation.
//!
//! This module provides a thread-safe LRU cache for SQL-to-RQL translations.
//! Repeated identical SQL queries will return cached results, improving performance.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{QuerySchema, SqlError, Translation, translate_with_schema};

/// Configuration for the SQL translation cache.
#[derive(Debug, Clone, Copy)]
pub struct CacheConfig {
    /// Maximum number of entries in the cache. Default: 1000.
    pub max_entries: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self { max_entries: 1000 }
    }
}

/// Statistics for cache performance monitoring.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    /// Number of entries currently in the cache.
    pub entries: usize,
    /// Total number of cache hits.
    pub hits: u64,
    /// Total number of cache misses.
    pub misses: u64,
}

impl CacheStats {
    /// Calculate hit rate as a percentage (0.0 to 100.0).
    #[must_use]
    pub const fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            (self.hits as f64 / total as f64) * 100.0
        }
    }
}

/// Internal cache structure.
struct TranslationCache {
    cache: HashMap<u64, Translation>,
    lru_queue: VecDeque<u64>,
    config: CacheConfig,
}

impl TranslationCache {
    fn new() -> Self {
        Self {
            cache: HashMap::new(),
            lru_queue: VecDeque::new(),
            config: CacheConfig::default(),
        }
    }

    fn get(&mut self, hash: u64) -> Option<Translation> {
        if let Some(translation) = self.cache.get(&hash) {
            // Move to back of LRU queue (most recently used)
            if let Some(pos) = self.lru_queue.iter().position(|&h| h == hash) {
                self.lru_queue.remove(pos);
                self.lru_queue.push_back(hash);
            }
            Some(translation.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, hash: u64, translation: Translation) {
        // If already present, remove from LRU queue
        if self.cache.contains_key(&hash)
            && let Some(pos) = self.lru_queue.iter().position(|&h| h == hash)
        {
            self.lru_queue.remove(pos);
        }

        // Evict LRU entry if at capacity
        while self.cache.len() >= self.config.max_entries {
            if let Some(lru_hash) = self.lru_queue.pop_front() {
                self.cache.remove(&lru_hash);
            } else {
                break;
            }
        }

        // Insert new entry
        self.cache.insert(hash, translation);
        self.lru_queue.push_back(hash);
    }

    fn clear(&mut self) {
        self.cache.clear();
        self.lru_queue.clear();
    }

    fn set_config(&mut self, config: CacheConfig) {
        self.config = config;
        // Evict entries if over new limit
        while self.cache.len() > config.max_entries {
            if let Some(lru_hash) = self.lru_queue.pop_front() {
                self.cache.remove(&lru_hash);
            } else {
                break;
            }
        }
    }

    fn len(&self) -> usize {
        self.cache.len()
    }
}

/// Global cache instance.
static CACHE: RwLock<Option<TranslationCache>> = RwLock::new(None);

/// Global cache statistics (atomic for lock-free reads).
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

fn hash_sql(sql: &str, schema: &QuerySchema) -> u64 {
    let mut hasher = DefaultHasher::new();
    sql.hash(&mut hasher);
    schema.version.hash(&mut hasher);
    hasher.finish()
}

fn ensure_cache_initialized(cache: &mut Option<TranslationCache>) {
    if cache.is_none() {
        *cache = Some(TranslationCache::new());
    }
}

/// Translate SQL with caching.
///
/// First checks the cache for a previously translated query with the same SQL string.
/// If found, returns the cached result (cache hit). Otherwise, translates the query
/// and stores the result in the cache (cache miss).
///
/// # Arguments
///
/// * `sql` - A SQL query string to translate.
///
/// # Returns
///
/// A [`Translation`] containing the RQL equivalent, or a [`SqlError`]
/// if the query is invalid or unsupported.
///
/// # Example
///
/// ```
/// use sql_parser::translate_cached;
///
/// let result = translate_cached("SELECT * FROM products WHERE price > 100").unwrap();
/// assert_eq!(result.index_name, "products");
/// ```
pub fn translate_cached(sql: &str) -> Result<Translation, SqlError> {
    translate_cached_with_schema(sql, &QuerySchema::default())
}

/// Translate SQL with caching and schema metadata.
pub fn translate_cached_with_schema(
    sql: &str,
    schema: &QuerySchema,
) -> Result<Translation, SqlError> {
    let hash = hash_sql(sql, schema);

    // Try to get from cache (read lock)
    {
        let cache_guard = CACHE.read().expect("Cache lock poisoned");
        if let Some(ref cache) = *cache_guard {
            // We need write access to update LRU order, so we just check existence
            if cache.cache.contains_key(&hash) {
                drop(cache_guard);
                // Upgrade to write lock
                let mut cache_guard = CACHE.write().expect("Cache lock poisoned");
                ensure_cache_initialized(&mut cache_guard);
                if let Some(result) = cache_guard.as_mut().unwrap().get(hash) {
                    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
                    return Ok(result);
                }
            }
        }
    }

    // Cache miss - translate
    let translation = translate_with_schema(sql, schema)?;

    // Store in cache (write lock)
    {
        let mut cache_guard = CACHE.write().expect("Cache lock poisoned");
        ensure_cache_initialized(&mut cache_guard);
        cache_guard
            .as_mut()
            .unwrap()
            .insert(hash, translation.clone());
    }

    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    Ok(translation)
}

/// Set the cache configuration.
///
/// This updates the cache settings. If the new `max_entries` is smaller than
/// the current number of entries, the least recently used entries are evicted.
pub fn set_cache_config(config: CacheConfig) {
    let mut cache_guard = CACHE.write().expect("Cache lock poisoned");
    ensure_cache_initialized(&mut cache_guard);
    cache_guard.as_mut().unwrap().set_config(config);
}

/// Clear all entries from the cache.
///
/// This removes all cached translations but preserves the configuration
/// and resets hit/miss statistics.
pub fn clear_cache() {
    let mut cache_guard = CACHE.write().expect("Cache lock poisoned");
    if let Some(ref mut cache) = *cache_guard {
        cache.clear();
    }
    CACHE_HITS.store(0, Ordering::Relaxed);
    CACHE_MISSES.store(0, Ordering::Relaxed);
}

/// Get current cache statistics.
///
/// Returns the number of entries, hits, and misses. The hit rate can be
/// calculated from these values.
#[must_use]
pub fn get_cache_stats() -> CacheStats {
    let cache_guard = CACHE.read().expect("Cache lock poisoned");
    let entries = cache_guard.as_ref().map_or(0, TranslationCache::len);
    CacheStats {
        entries,
        hits: CACHE_HITS.load(Ordering::Relaxed),
        misses: CACHE_MISSES.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldCapabilities;
    use std::sync::Mutex;

    // Mutex to ensure tests run serially when they share the global cache
    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    fn with_clean_cache<T>(f: impl FnOnce() -> T) -> T {
        let _guard = TEST_MUTEX.lock().expect("Test mutex poisoned");
        clear_cache();
        set_cache_config(CacheConfig::default());
        f()
    }

    #[test]
    fn test_cache_hit() {
        with_clean_cache(|| {
            let sql = "SELECT * FROM test_hit_idx WHERE price > 100";

            // First call - cache miss
            let result1 = translate_cached(sql).unwrap();
            let stats1 = get_cache_stats();
            assert_eq!(stats1.misses, 1);
            assert_eq!(stats1.hits, 0);

            // Second call - cache hit
            let result2 = translate_cached(sql).unwrap();
            let stats2 = get_cache_stats();
            assert_eq!(stats2.hits, 1);
            assert_eq!(stats2.misses, 1);

            // Results should be equivalent
            assert_eq!(result1.index_name, result2.index_name);
            assert_eq!(result1.query_string, result2.query_string);
        });
    }

    #[test]
    fn test_cache_miss_different_queries() {
        with_clean_cache(|| {
            translate_cached("SELECT * FROM miss_idx1").unwrap();
            translate_cached("SELECT * FROM miss_idx2").unwrap();

            let stats = get_cache_stats();
            assert_eq!(stats.misses, 2);
            assert_eq!(stats.hits, 0);
            assert_eq!(stats.entries, 2);
        });
    }

    #[test]
    fn test_cache_eviction() {
        with_clean_cache(|| {
            set_cache_config(CacheConfig { max_entries: 2 });

            translate_cached("SELECT * FROM evict_idx1").unwrap();
            translate_cached("SELECT * FROM evict_idx2").unwrap();
            translate_cached("SELECT * FROM evict_idx3").unwrap(); // Should evict oldest

            let stats = get_cache_stats();
            assert_eq!(stats.entries, 2);
        });
    }

    #[test]
    fn test_cache_clear() {
        with_clean_cache(|| {
            translate_cached("SELECT * FROM clear_idx").unwrap();
            assert!(get_cache_stats().entries >= 1);

            clear_cache();
            let stats = get_cache_stats();
            assert_eq!(stats.entries, 0);
            assert_eq!(stats.hits, 0);
            assert_eq!(stats.misses, 0);
        });
    }

    #[test]
    fn test_hit_rate() {
        let stats = CacheStats {
            entries: 10,
            hits: 75,
            misses: 25,
        };
        assert!((stats.hit_rate() - 75.0).abs() < f64::EPSILON);

        let empty_stats = CacheStats::default();
        assert_eq!(empty_stats.hit_rate(), 0.0);
    }

    #[test]
    fn test_invalid_sql_not_cached() {
        with_clean_cache(|| {
            let entries_before = get_cache_stats().entries;

            let result = translate_cached("INVALID SQL");
            assert!(result.is_err());

            // Invalid queries should not add entries
            let stats = get_cache_stats();
            assert_eq!(stats.entries, entries_before);
        });
    }

    #[test]
    fn test_schema_version_in_cache_key() {
        with_clean_cache(|| {
            let sql = "SELECT * FROM cache_schema_idx WHERE category = 'books'";
            let schema_v1 = QuerySchema::new(1).with_field("category", FieldCapabilities::tag());
            let schema_v2 = QuerySchema::new(2).with_field("category", FieldCapabilities::tag());

            translate_cached_with_schema(sql, &schema_v1).unwrap();
            translate_cached_with_schema(sql, &schema_v1).unwrap();
            translate_cached_with_schema(sql, &schema_v2).unwrap();

            let stats = get_cache_stats();
            assert_eq!(stats.hits, 1);
            assert_eq!(stats.misses, 2);
            assert_eq!(stats.entries, 2);
        });
    }
}
