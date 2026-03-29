/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Query result cache for RediSearch.
//!
//! Provides an LRU cache for query results to avoid re-executing identical queries
//! on unchanged data. The cache is keyed by a hash of query parameters and index
//! revision, ensuring cached results are invalidated when the index changes.

use ahash::AHashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// A cache key combining query hash and index revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey {
    /// Hash of (index_name, query_string, limit, offset, sort_params)
    pub query_hash: u64,
    /// Index revision number (bumped on writes)
    pub index_revision: u64,
}

/// Cached query result data.
#[derive(Debug, Clone)]
pub struct CachedResult {
    /// Serialized result data (format TBD - could be msgpack, bincode, etc.)
    pub data: Vec<u8>,
    /// Size in bytes for memory tracking
    pub size_bytes: usize,
}

/// Statistics for cache performance monitoring.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheStats {
    /// Total number of cache lookups
    pub lookups: u64,
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of entries evicted due to size limit
    pub evictions: u64,
    /// Current number of entries in cache
    pub entries: usize,
    /// Total memory used by cached data (bytes)
    pub memory_bytes: usize,
}

impl CacheStats {
    /// Calculate hit rate as a percentage (0.0 to 100.0).
    #[must_use]
    pub const fn hit_rate(&self) -> f64 {
        if self.lookups == 0 {
            0.0
        } else {
            (self.hits as f64 / self.lookups as f64) * 100.0
        }
    }
}

/// LRU cache for query results.
///
/// Thread-safe via interior mutability (uses atomics for stats).
/// The cache itself is not thread-safe and should be protected by a mutex
/// when accessed from multiple threads.
pub struct QueryCache {
    /// Maximum number of entries
    max_entries: usize,
    /// Cache storage
    cache: AHashMap<CacheKey, CachedResult>,
    /// LRU queue (most recently used at back)
    lru_queue: VecDeque<CacheKey>,
    /// Statistics (atomic for lock-free reads)
    lookups: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    memory_bytes: AtomicUsize,
}

impl QueryCache {
    /// Create a new query cache with the specified maximum number of entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use query_cache::QueryCache;
    ///
    /// let cache = QueryCache::new(1000);
    /// assert_eq!(cache.max_entries(), 1000);
    /// ```
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            cache: AHashMap::with_capacity(max_entries),
            lru_queue: VecDeque::with_capacity(max_entries),
            lookups: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            memory_bytes: AtomicUsize::new(0),
        }
    }

    /// Get the maximum number of entries this cache can hold.
    #[must_use]
    pub const fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Get a cached result if it exists.
    ///
    /// Updates LRU order and statistics.
    pub fn get(&mut self, key: &CacheKey) -> Option<&CachedResult> {
        self.lookups.fetch_add(1, Ordering::Relaxed);

        if let Some(result) = self.cache.get(key) {
            // Move to back of LRU queue (most recently used)
            if let Some(pos) = self.lru_queue.iter().position(|k| k == key) {
                self.lru_queue.remove(pos);
                self.lru_queue.push_back(*key);
            }

            self.hits.fetch_add(1, Ordering::Relaxed);
            Some(result)
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Insert a result into the cache.
    ///
    /// Evicts the least recently used entry if the cache is full.
    pub fn insert(&mut self, key: CacheKey, result: CachedResult) {
        // If key already exists, remove it first to update memory tracking
        if let Some(old_result) = self.cache.remove(&key) {
            self.memory_bytes
                .fetch_sub(old_result.size_bytes, Ordering::Relaxed);
            if let Some(pos) = self.lru_queue.iter().position(|k| k == &key) {
                self.lru_queue.remove(pos);
            }
        }

        // Evict LRU entry if at capacity
        if self.cache.len() >= self.max_entries
            && let Some(lru_key) = self.lru_queue.pop_front()
            && let Some(evicted) = self.cache.remove(&lru_key)
        {
            self.memory_bytes
                .fetch_sub(evicted.size_bytes, Ordering::Relaxed);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }

        // Insert new entry
        self.memory_bytes
            .fetch_add(result.size_bytes, Ordering::Relaxed);
        self.cache.insert(key, result);
        self.lru_queue.push_back(key);
    }

    /// Clear all entries from the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.lru_queue.clear();
        self.memory_bytes.store(0, Ordering::Relaxed);
    }

    /// Get current cache statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            lookups: self.lookups.load(Ordering::Relaxed),
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            entries: self.cache.len(),
            memory_bytes: self.memory_bytes.load(Ordering::Relaxed),
        }
    }

    /// Reset statistics counters (but keep cached entries).
    pub fn reset_stats(&mut self) {
        self.lookups.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
        self.misses.store(0, Ordering::Relaxed);
        self.evictions.store(0, Ordering::Relaxed);
    }

    /// Resize the cache to a new maximum number of entries.
    ///
    /// If the new size is smaller than the current number of entries,
    /// evicts the least recently used entries.
    pub fn resize(&mut self, new_max_entries: usize) {
        self.max_entries = new_max_entries;

        // Evict entries if we're over the new limit
        while self.cache.len() > new_max_entries
            && let Some(lru_key) = self.lru_queue.pop_front()
            && let Some(evicted) = self.cache.remove(&lru_key)
        {
            self.memory_bytes
                .fetch_sub(evicted.size_bytes, Ordering::Relaxed);
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Opaque wrapper for FFI.
pub mod opaque {
    use super::QueryCache;

    /// Opaque type for passing `QueryCache` across FFI boundary.
    #[repr(C)]
    pub struct OpaqueQueryCache {
        _private: [u8; 0],
    }

    impl QueryCache {
        /// Convert to opaque pointer for FFI.
        #[must_use]
        pub fn into_opaque(self: Box<Self>) -> *mut OpaqueQueryCache {
            Box::into_raw(self).cast()
        }

        /// Convert from opaque pointer (unsafe - caller must ensure validity).
        ///
        /// # Safety
        ///
        /// - `ptr` must have been created by `into_opaque`
        /// - `ptr` must not have been freed
        /// - `ptr` must not be used after this call (ownership is transferred)
        #[must_use]
        pub unsafe fn from_opaque(ptr: *mut OpaqueQueryCache) -> Box<Self> {
            // SAFETY: Caller guarantees ptr was created by into_opaque and is still valid
            unsafe { Box::from_raw(ptr.cast()) }
        }

        /// Borrow from opaque pointer (unsafe - caller must ensure validity).
        ///
        /// # Safety
        ///
        /// - `ptr` must have been created by `into_opaque`
        /// - `ptr` must not have been freed
        /// - No mutable references to the same cache must exist
        #[must_use]
        pub const unsafe fn from_opaque_ref<'a>(ptr: *const OpaqueQueryCache) -> &'a Self {
            // SAFETY: Caller guarantees ptr is valid and no mutable refs exist
            unsafe { &*ptr.cast() }
        }

        /// Mutably borrow from opaque pointer (unsafe - caller must ensure validity).
        ///
        /// # Safety
        ///
        /// - `ptr` must have been created by `into_opaque`
        /// - `ptr` must not have been freed
        /// - No other references to the same cache must exist
        #[must_use]
        pub unsafe fn from_opaque_mut<'a>(ptr: *mut OpaqueQueryCache) -> &'a mut Self {
            // SAFETY: Caller guarantees ptr is valid and no other refs exist
            unsafe { &mut *ptr.cast() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_cache_operations() {
        let mut cache = QueryCache::new(2);

        let key1 = CacheKey {
            query_hash: 1,
            index_revision: 1,
        };
        let key2 = CacheKey {
            query_hash: 2,
            index_revision: 1,
        };

        let result1 = CachedResult {
            data: vec![1, 2, 3],
            size_bytes: 3,
        };
        let result2 = CachedResult {
            data: vec![4, 5, 6],
            size_bytes: 3,
        };

        // Insert and retrieve
        cache.insert(key1, result1.clone());
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_none());

        // Check stats
        let stats = cache.stats();
        assert_eq!(stats.lookups, 2);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.entries, 1);
        assert_eq!(stats.memory_bytes, 3);

        // Insert second entry
        cache.insert(key2, result2);
        assert_eq!(cache.stats().entries, 2);
        assert_eq!(cache.stats().memory_bytes, 6);
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = QueryCache::new(2);

        let key1 = CacheKey {
            query_hash: 1,
            index_revision: 1,
        };
        let key2 = CacheKey {
            query_hash: 2,
            index_revision: 1,
        };
        let key3 = CacheKey {
            query_hash: 3,
            index_revision: 1,
        };

        let result = CachedResult {
            data: vec![1],
            size_bytes: 1,
        };

        // Fill cache
        cache.insert(key1, result.clone());
        cache.insert(key2, result.clone());

        // Access key1 to make it more recently used
        assert!(cache.get(&key1).is_some());

        // Insert key3, should evict key2 (least recently used)
        cache.insert(key3, result);

        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_none());
        assert!(cache.get(&key3).is_some());

        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
        assert_eq!(stats.entries, 2);
    }

    #[test]
    fn test_cache_clear() {
        let mut cache = QueryCache::new(10);

        let key = CacheKey {
            query_hash: 1,
            index_revision: 1,
        };
        let result = CachedResult {
            data: vec![1, 2, 3],
            size_bytes: 3,
        };

        cache.insert(key, result);
        assert_eq!(cache.stats().entries, 1);

        cache.clear();
        assert_eq!(cache.stats().entries, 0);
        assert_eq!(cache.stats().memory_bytes, 0);
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn test_cache_resize() {
        let mut cache = QueryCache::new(3);

        for i in 0..3 {
            let key = CacheKey {
                query_hash: i,
                index_revision: 1,
            };
            let result = CachedResult {
                data: vec![i as u8],
                size_bytes: 1,
            };
            cache.insert(key, result);
        }

        assert_eq!(cache.stats().entries, 3);

        // Resize to smaller capacity
        cache.resize(1);
        assert_eq!(cache.stats().entries, 1);
        assert_eq!(cache.stats().evictions, 2);
    }

    #[test]
    fn test_hit_rate_calculation() {
        let stats = CacheStats {
            lookups: 100,
            hits: 75,
            misses: 25,
            evictions: 0,
            entries: 10,
            memory_bytes: 1000,
        };

        assert!((stats.hit_rate() - 75.0).abs() < f64::EPSILON);

        // Test zero lookups
        let empty_stats = CacheStats::default();
        assert_eq!(empty_stats.hit_rate(), 0.0);
    }
}
