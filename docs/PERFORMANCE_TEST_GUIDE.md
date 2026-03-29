# Performance Optimizations - Manual Test Guide

## Overview

This guide covers the current audited performance workstreams only:

- Query Cache v2
- Bloom Filter v2
- Numeric Tree compaction

`src/redisearch_rs/datetime/` is valid feature work, but it is not part of the
performance program and is intentionally excluded from this guide. Cursor
Adaptive default is also not part of the shipping performance matrix: the
explicit `WITHCURSOR ADAPTIVE` query flag exists, but
`CURSOR_ADAPTIVE_DEFAULT` remains parked with no measured justification for
turning it on by default.

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

## 1. Query Cache v2

### What's New

- Replayable cached `FT.SEARCH` results with scores and row data
- Safer request fingerprinting for response-shaping inputs
- Cache stats in `FT.INFO`

### Configuration

```bash
# Check current settings
redis-cli FT.CONFIG GET QUERYCACHE_ENABLED
redis-cli FT.CONFIG GET QUERYCACHE_MAX_SIZE

# Change cache settings
redis-cli FT.CONFIG SET QUERYCACHE_ENABLED true
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

# Run the same query twice
redis-cli FT.SEARCH products "@price:[500 inf]"
redis-cli FT.SEARCH products "@price:[500 inf]"

# Check cache stats
redis-cli FT.INFO products | grep -A 12 "global_query_cache_stats"
```

### Expected Output

- `FT.INFO` includes a `global_query_cache_stats` block
- `hits` increases on the repeated query
- `misses` records the cold query
- `entries` and related stats are visible under `global_query_cache_stats`

---

## 2. Bloom Filter v2

### What's New

- Fast negative-term rejection using the real `BLOOM_FILTER_ENABLED` toggle
- Capacity tracking, rebuild behavior, and `FT.INFO` bloom stats

### Configuration

```bash
# Check if enabled
redis-cli FT.CONFIG GET BLOOM_FILTER_ENABLED

# Toggle runtime behavior
redis-cli FT.CONFIG SET BLOOM_FILTER_ENABLED true
```

### Test Steps

```bash
# Create index
redis-cli FT.CREATE articles SCHEMA title TEXT body TEXT

# Add documents
redis-cli HSET art1 title "Redis Guide" body "Learn Redis basics"
redis-cli HSET art2 title "Search Tutorial" body "Full-text search guide"

# Search for an existing term
redis-cli FT.SEARCH articles "Redis"

# Search for a non-existing term
redis-cli FT.SEARCH articles "xyznonexistent123"

# Inspect bloom stats
redis-cli FT.INFO articles | grep -A 12 "bloom"
```

### What to Look For

- Correct results with bloom both on and off
- Bloom stats such as capacity, items, estimated FPR, and memory usage
- Faster rejection for absent terms when bloom is enabled

---

## 3. Numeric Tree Compaction

### What's New

- Adaptive compaction for sparse numeric data
- Structural metrics and reclaimed-memory reporting
- Runtime toggle support for the productionized compaction path

### Test Steps

```bash
# Create index with numeric field
redis-cli FT.CREATE metrics SCHEMA value NUMERIC

# Add sparse numeric data (large gaps)
redis-cli HSET m1 value 100
redis-cli HSET m2 value 500
redis-cli HSET m3 value 10000
redis-cli HSET m4 value 50000

# Inspect index stats
redis-cli FT.INFO metrics

# Query ranges
redis-cli FT.SEARCH metrics "@value:[0 1000]"
redis-cli FT.SEARCH metrics "@value:[10000 inf]"
```

### What to Look For

- Correct range-query results before and after GC/compaction activity
- Compaction/memory stats that explain reclaimed structure instead of relying on inference

---

## 4. Parked / Out-of-scope Items

- **DATETIME**: feature roadmap work, not a performance initiative. Validate it via
  `tests/pytests/test_datetime_field.py`, not this guide.
- **Cursor Adaptive default**: `CURSOR_ADAPTIVE_DEFAULT` remains `false`. Use
  `tests/benchmarks/bench_cursor_adaptive.py` only to confirm parked status and
  rationale, not to claim delivered performance value.

---

## Cleanup

```bash
redis-cli FT.DROPINDEX products DD
redis-cli FT.DROPINDEX articles DD
redis-cli FT.DROPINDEX metrics DD
redis-cli SHUTDOWN NOSAVE
```

---

## Summary Table

| Feature | Command to Test | What to Look For |
|---------|-----------------|------------------|
| Query Cache v2 | `FT.INFO <idx>` after repeated `FT.SEARCH` | `global_query_cache_stats` hits/misses/entries |
| Bloom Filter v2 | Search non-existing term + `FT.INFO` | fast rejection plus bloom stats |
| Numeric Tree compaction | `FT.INFO <idx>` + sparse range queries | compaction/memory stats with correct results |
| Cursor Adaptive default | `python3 tests/benchmarks/bench_cursor_adaptive.py` | parked status and rationale, not a performance claim |

