# Query Cache Phase 2: Field Value Loading

## Executive Summary

Phase 1 of the query cache successfully caches document IDs for repeated queries. However, cached results return **empty field values** because the loader processor (which fetches field data from Redis) is bypassed when returning cached results. Phase 2 addresses this gap.

---

## 1. Problem Statement

### Current Behavior (Phase 1)

1. **Cache Storage**: The cache stores only document IDs (`t_docId` array via `CachedDocIds` struct)
2. **Cache Hit Path**: On cache hit, `QueryCache_DeserializeDocIds` reconstructs `SearchResult` objects containing:
   - Document ID (`doc_id`)
   - Document metadata (`RSDocumentMetadata` from `DocTable_Borrow`)
   - **No field values** (RLookupRow is empty)
3. **Bypassed Pipeline**: The result processor pipeline (specifically `RPLoader`) is never invoked for cached results
4. **Result**: Queries return correct document IDs but with empty/missing field values

### Code Flow Comparison

**Normal Query (no cache)**:
```
Query Parser → Iterator → RP_LOADER → RP_SCORER → RP_SORTER → Serializer
                              ↑
                    Loads fields from Redis
```

**Cached Query (current)**:
```
Cache Lookup → DeserializeDocIds → Serializer
                                        ↑
                              No field loading!
```

### Impact

- `FT.SEARCH idx "hello" RETURN 1 title` returns `[1, "doc:1", []]` instead of `[1, "doc:1", ["title", "Hello World"]]`
- All RETURN, LOAD, and HIGHLIGHT operations produce empty results on cache hits
- Users see inconsistent behavior between first query (correct) and subsequent queries (empty fields)

---

## 2. Design Options

### Option A: Cache Full Results (Field Values Included)

**Approach**: Serialize complete field values into the cache alongside document IDs.

**Cache Format**:
```c
typedef struct CachedFullResult {
    uint64_t count;
    uint64_t total_size;
    CachedDocument docs[];  // Variable length
} CachedFullResult;

typedef struct CachedDocument {
    t_docId doc_id;
    float score;
    uint32_t num_fields;
    CachedField fields[];   // Variable length
} CachedDocument;

typedef struct CachedField {
    uint32_t key_len;
    uint32_t value_len;
    char data[];            // key + value concatenated
} CachedField;
```

**Pros**:
- Fastest cache hits (no Redis calls whatsoever)
- Complete query bypass - maximum throughput improvement
- Consistent latency for cached queries

**Cons**:
- **High memory usage**: Field values can be large (KB-MB per document)
- **Cache invalidation complexity**: Field updates don't bump index revision
- **Stale data risk**: Cached field values may not reflect current Redis state
- **RETURN clause dependency**: Different RETURN clauses for same query can't share cache entry

**Performance Characteristics**:
- Memory: 10-100x higher than doc ID caching
- Latency: Near-zero for cache hits
- Throughput: Highest possible (limited only by network)

---

### Option B: Run Loader on Cached Doc IDs (Recommended)

**Approach**: On cache hit, inject cached doc IDs into a minimal result processor pipeline that includes the loader.

**Implementation**:
```
Cache Lookup → DeserializeDocIds → RPLoader → Serializer
                                       ↑
                             Load fields from Redis
```

**Key Changes**:
1. Create `RPCachedResultsSource` - a result processor that yields cached results
2. On cache hit, build minimal pipeline: `RPCachedResultsSource → RPLoader → RPHighlighter (optional)`
3. Reuse existing loader infrastructure for field loading

**New Result Processor**:
```c
typedef struct RPCachedResultsSource {
    ResultProcessor base;
    SearchResult **results;
    size_t count;
    size_t current;
} RPCachedResultsSource;

static int rpCachedResultsNext(ResultProcessor *base, SearchResult *r) {
    RPCachedResultsSource *src = (RPCachedResultsSource *)base;
    if (src->current >= src->count) {
        return RS_RESULT_EOF;
    }
    *r = *src->results[src->current++];  // Move result
    return RS_RESULT_OK;
}
```

**Pros**:
- **Low memory overhead**: Only doc IDs cached (current behavior)
- **Always fresh data**: Field values loaded from Redis at query time
- **Simpler invalidation**: Only index structure changes need cache invalidation
- **RETURN clause flexibility**: Same cached doc IDs work for different field selections
- **Leverages existing code**: Reuses battle-tested loader infrastructure

**Cons**:
- Still requires Redis calls on cache hit (but much faster than full query)
- Loader is single-threaded per query

**Performance Characteristics**:
- Memory: Same as Phase 1 (minimal)
- Latency: ~50-80% of full query (skip parsing, planning, index traversal)
- Throughput: 5-10x improvement (query execution is typically >50% of latency)

---

### Option C: Hybrid Approach (Cache Hot Fields)

**Approach**: Cache doc IDs + frequently accessed fields. Run loader only for non-cached fields.

**Cache Format**:
```c
typedef struct HybridCachedResult {
    uint64_t count;
    uint64_t cached_field_mask;  // Bitmap of which fields are cached
    CachedDocWithFields docs[];
} HybridCachedResult;
```

**Pros**:
- Balance between memory and latency
- Hot fields (e.g., `title`, `score`) served from cache
- Rare fields still loadable on-demand

**Cons**:
- **High complexity**: Field presence tracking, partial cache hits
- **Configuration burden**: Which fields to cache?
- **Diminishing returns**: Most of the benefit comes from skipping query execution, not field loading

**Performance Characteristics**:
- Memory: 2-10x Phase 1 (depends on cached fields)
- Latency: Marginal improvement over Option B
- Throughput: Similar to Option B

---

## 3. Recommended Approach: Option B

### Justification

1. **Memory Efficiency**: Production deployments are often memory-constrained. Option B adds zero memory overhead.

2. **Data Freshness**: Field values can change without index updates (e.g., direct Redis SET). Option B always returns current data.

3. **Simplicity**: Option B requires ~100-200 LOC. Option A/C require 500-1000+ LOC for serialization, field tracking, and invalidation.

4. **Performance Sweet Spot**: Query parsing + index traversal typically accounts for 60-80% of query latency. Option B eliminates this while keeping implementation simple.

5. **No New Invalidation Logic**: Option A would require tracking field modifications, which RediSearch doesn't currently do.

---

## 4. Implementation Plan

### Phase 2a: Minimal Pipeline for Cached Results (MVP)

**Effort**: 2-3 days

1. **Create `RPCachedResultsSource`** (~50 LOC)
   - New result processor that yields pre-loaded `SearchResult` objects
   - File: `src/result_processor.c`

2. **Modify cache hit path** (~50 LOC)
   - Build minimal pipeline: `RPCachedResultsSource → RPLoader`
   - Wire up in `sendChunk_Resp2` and `sendChunk_Resp3`
   - File: `src/aggregate/aggregate_exec.c`

3. **Extract loader setup** (~30 LOC)
   - Refactor loader creation to be callable outside normal pipeline build
   - File: `src/aggregate/aggregate_plan.c`

### Phase 2b: Optimizations (Optional)

**Effort**: 1-2 days per item

1. **Batch loader optimization**: Load all doc fields in single Redis call
2. **Skip loader for NOCONTENT**: If `NOCONTENT` flag, skip loader entirely
3. **Parallel field loading**: Use Redis pipelining for multiple docs

### Testing Plan

1. Extend `test_query_cache.py` with field value assertions
2. Test with various RETURN clauses
3. Test HIGHLIGHT and SUMMARIZE with cached results
4. Memory/latency benchmarks

---

## 5. Performance Expectations

| Metric | Phase 1 (Current) | Phase 2 (Option B) | Option A (Full Cache) |
|--------|-------------------|--------------------|-----------------------|
| Cache Hit Latency | N/A (broken) | 20-40% of full query | 5-10% of full query |
| Memory Overhead | ~40 bytes/doc | ~40 bytes/doc | 1-10 KB/doc |
| Throughput Improvement | N/A | 3-5x | 10-20x |
| Implementation Complexity | ✅ Done | ✅ Low | ⚠️ High |

### Benchmarking Commands

```bash
# Baseline (no cache)
redis-benchmark -n 10000 FT.SEARCH idx "@field:value" RETURN 2 title body

# With cache (Phase 2)
FT.CONFIG SET QUERYCACHE_ENABLED true
redis-benchmark -n 10000 FT.SEARCH idx "@field:value" RETURN 2 title body
```

---

## 6. Migration/Rollout Strategy

1. **Phase 2 behind existing flag**: `QUERYCACHE_ENABLED` already controls the feature
2. **No schema changes**: Cache format unchanged (still doc IDs only)
3. **Gradual rollout**: 
   - Test in staging with representative queries
   - Monitor cache hit ratio via `FT.INFO`
   - Enable in production with monitoring

### Rollback

If issues arise, `FT.CONFIG SET QUERYCACHE_ENABLED false` immediately disables caching.

---

## Appendix: Alternative Considered - Query Re-execution

**Approach**: On cache hit, re-execute query but use cached doc IDs as filter.

**Rejected because**:
- Still executes query parsing and planning
- Minimal benefit over Option B
- More complex implementation

