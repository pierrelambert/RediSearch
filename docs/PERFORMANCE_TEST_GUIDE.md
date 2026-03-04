# Performance Optimizations - Manual Test Guide

## Overview
Quick guide to manually test and visualize the new performance features in PR #2.

## Prerequisites
- Redis server with RediSearch module loaded
- Redis CLI

## Quick Start
```bash
# Build the module
./build.sh

# Start Redis with module
redis-server --loadmodule bin/macos-aarch64-release/search-community/redisearch.so &
sleep 3
redis-cli PING
```

---

## 1. Query Result Cache

### What's New
- LRU cache for query results
- Cache stats in FT.INFO

### Configuration
```bash
# Check current setting
redis-cli FT.CONFIG GET QUERYCACHE_MAX_SIZE

# Change cache size
redis-cli FT.CONFIG SET QUERYCACHE_MAX_SIZE 2000
```

### Test Steps
```bash
# Create test index
redis-cli FT.CREATE products SCHEMA name TEXT price NUMERIC

# Add test data
redis-cli HSET doc1 name "Laptop" price 999
redis-cli HSET doc2 name "Phone" price 599
redis-cli HSET doc3 name "Tablet" price 399

# Run same query twice
redis-cli FT.SEARCH products "@price:[500 inf]"
redis-cli FT.SEARCH products "@price:[500 inf]"

# Check cache stats
redis-cli FT.INFO products | grep -A 10 "query_cache"
```

### Expected Output
- `query_cache_hits`: Should show hits after repeated queries
- `query_cache_misses`: First query causes miss
- `query_cache_entries`: Number of cached queries

---

## 2. Bloom Filter

### What's New
- Fast O(1) term rejection
- Reduces unnecessary index lookups

### Configuration
```bash
# Check if enabled
redis-cli FT.CONFIG GET BLOOM_FILTER_ENABLED

# Toggle (if supported)
redis-cli FT.CONFIG SET BLOOM_FILTER_ENABLED true
```

### Test Steps
```bash
# Create index
redis-cli FT.CREATE articles SCHEMA title TEXT body TEXT

# Add documents
redis-cli HSET art1 title "Redis Guide" body "Learn Redis basics"
redis-cli HSET art2 title "Search Tutorial" body "Full-text search guide"

# Search for existing term (should find)
redis-cli FT.SEARCH articles "Redis"

# Search for non-existing term (bloom filter rejects fast)
redis-cli FT.SEARCH articles "xyznonexistent123"
```

### How to Visualize
- Non-existing term queries should return instantly
- Bloom filter prevents unnecessary index traversal

---

## 3. DateTime Field Type

### What's New
- Native DATETIME field type
- ISO-8601 parsing
- Range queries with dates

### Test Steps
```bash
# Create index with DATETIME field
redis-cli FT.CREATE events SCHEMA title TEXT created DATETIME

# Add events with ISO-8601 dates
redis-cli HSET evt1 title "Conference" created "2024-03-15T10:00:00Z"
redis-cli HSET evt2 title "Meeting" created "2024-06-20T14:30:00Z"
redis-cli HSET evt3 title "Workshop" created "2024-12-01T09:00:00Z"

# Query date ranges
redis-cli FT.SEARCH events "@created:[2024-03-01T00:00:00Z 2024-06-30T23:59:59Z]"

# Sort by date
redis-cli FT.SEARCH events "*" SORTBY created ASC
```

### Expected Output
- Date range queries filter correctly
- Sorting by date works

---

## 4. Cursor Adaptive Mode

### What's New
- ADAPTIVE mode for consistent ~10ms response chunks
- Better than fixed COUNT for variable data

### Test Steps
```bash
# Create index with many documents
redis-cli FT.CREATE bigindex SCHEMA data TEXT

# Add 1000 test documents
for i in {1..1000}; do
  redis-cli HSET doc$i data "test data $i"
done

# Use ADAPTIVE cursor (auto-adjusts chunk size)
redis-cli FT.AGGREGATE bigindex "*" WITHCURSOR ADAPTIVE

# Compare with fixed COUNT
redis-cli FT.AGGREGATE bigindex "*" WITHCURSOR COUNT 100
```

### How to Visualize
- ADAPTIVE aims for consistent ~10ms response times
- Fixed COUNT may have variable response times

---

## 5. Numeric Range Tree Memory Optimization

### What's New
- Adaptive compaction for sparse numeric data
- Sibling node merging
- 30-50% memory reduction for sparse data

### Test Steps
```bash
# Create index with numeric field
redis-cli FT.CREATE metrics SCHEMA value NUMERIC

# Add sparse numeric data (large gaps)
redis-cli HSET m1 value 100
redis-cli HSET m2 value 500
redis-cli HSET m3 value 10000
redis-cli HSET m4 value 50000

# Check index info
redis-cli FT.INFO metrics

# Query ranges
redis-cli FT.SEARCH metrics "@value:[0 1000]"
redis-cli FT.SEARCH metrics "@value:[10000 inf]"
```

### How to Visualize
- Memory usage in FT.INFO should be efficient for sparse data

---

## Cleanup
```bash
redis-cli FT.DROPINDEX products DD
redis-cli FT.DROPINDEX articles DD
redis-cli FT.DROPINDEX events DD
redis-cli FT.DROPINDEX bigindex DD
redis-cli FT.DROPINDEX metrics DD
redis-cli SHUTDOWN NOSAVE
```

---

## Summary Table

| Feature | Command to Test | What to Look For |
|---------|-----------------|------------------|
| Query Cache | `FT.INFO <idx>` | query_cache_hits/misses |
| Bloom Filter | Search non-existing term | Fast rejection |
| DateTime | `@created:[date1 date2]` | Date range filtering |
| Cursor Adaptive | `WITHCURSOR ADAPTIVE` | Consistent ~10ms chunks |
| Numeric Tree | FT.INFO memory stats | Efficient memory usage |

