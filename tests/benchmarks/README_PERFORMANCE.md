# Performance Benchmarks

This directory contains benchmark scripts to measure and validate performance improvements in RediSearch.

## Prerequisites

1. **Redis Server**: A running Redis server with RediSearch module loaded
2. **Python Dependencies**: Install required packages:
   ```bash
   pip install redis
   ```

## Benchmark Scripts

### 1. Query Cache Benchmark (`bench_query_cache.py`)

Measures throughput improvement when query cache is enabled vs disabled.

**Target**: 10x improvement for identical queries

**Usage**:
```bash
cd tests/benchmarks
python bench_query_cache.py
```

**What it measures**:
- Queries per second with cache disabled
- Queries per second with cache enabled
- Improvement ratio

**Pass criteria**: ≥10x improvement

---

### 2. Bloom Filter Benchmark (`bench_bloom_filter.py`)

Measures latency reduction for queries on non-existent terms.

**Target**: 50% reduction vs without bloom filter

**Usage**:
```bash
cd tests/benchmarks
python bench_bloom_filter.py
```

**What it measures**:
- Query latency without bloom filter
- Query latency with bloom filter
- Latency reduction percentage

**Pass criteria**: ≥50% reduction

---

### 3. Numeric Tree Benchmark (`bench_numeric_tree.py`)

Measures memory usage reduction with sparse numeric data.

**Target**: 30-50% reduction for sparse numeric distributions

**Usage**:
```bash
cd tests/benchmarks
python bench_numeric_tree.py
```

**What it measures**:
- Memory usage for dense numeric index
- Memory usage for sparse numeric index
- Memory reduction percentage

**Pass criteria**: 30-50% reduction

---

### 4. Cursor Adaptive Benchmark (`bench_cursor_adaptive.py`)

Measures average chunk processing time with ADAPTIVE cursor mode.

**Target**: Consistent ~10ms per chunk

**Usage**:
```bash
cd tests/benchmarks
python bench_cursor_adaptive.py
```

**What it measures**:
- Average chunk processing time
- Consistency (coefficient of variation)
- P99 latency

**Pass criteria**: 5-15ms average, CV < 50%

---

## Running All Benchmarks

### Using the Test Runner (Recommended)

```bash
cd tests/benchmarks
./run_all_benchmarks.sh
```

The test runner will:
- Check if Redis is running
- Run all benchmarks sequentially
- Print a summary of results
- Exit with code 0 if all pass, 1 if any fail

### Manual Execution

To run all benchmarks manually:

```bash
cd tests/benchmarks
for bench in bench_*.py; do
    echo "Running $bench..."
    python "$bench"
    echo ""
done
```

## Output Format

Each benchmark outputs:
- **Before/after comparison**: Metrics with and without the optimization
- **% improvement**: Percentage improvement or reduction
- **Pass/fail**: Whether the target was achieved

Example output:
```
=== Results ===
Cache disabled: 2.450ms
Cache enabled:  0.245ms
Improvement:    10.00x
Target:         10.00x

✓ PASS: Achieved target improvement
```

## Exit Codes

- `0`: Benchmark passed (target achieved)
- `1`: Benchmark failed (target not achieved or error occurred)

## Notes

- Benchmarks require a clean Redis instance for accurate results
- Each benchmark creates and destroys its own test indices
- Results may vary based on hardware and Redis configuration
- For production validation, run multiple times and average results

