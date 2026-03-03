#!/usr/bin/env python3
"""
Query Cache Benchmark

Measures throughput improvement when query cache is enabled vs disabled.
Target: 10x improvement for identical queries.
"""

import redis
import time
import statistics
import sys


def benchmark(name, func, iterations=1000):
    """Run a benchmark and collect timing statistics."""
    times = []
    for _ in range(iterations):
        start = time.perf_counter()
        func()
        times.append(time.perf_counter() - start)
    
    mean = statistics.mean(times) * 1000
    median = statistics.median(times) * 1000
    p99 = sorted(times)[int(len(times) * 0.99)] * 1000
    
    print(f"{name}:")
    print(f"  Mean: {mean:.3f}ms")
    print(f"  P50:  {median:.3f}ms")
    print(f"  P99:  {p99:.3f}ms")
    
    return mean


def setup_index(r):
    """Create test index and populate with data."""
    # Drop index if exists
    try:
        r.execute_command('FT.DROPINDEX', 'testidx', 'DD')
    except redis.ResponseError:
        pass
    
    # Create index
    r.execute_command(
        'FT.CREATE', 'testidx',
        'ON', 'HASH',
        'PREFIX', '1', 'doc:',
        'SCHEMA',
        'title', 'TEXT',
        'body', 'TEXT',
        'category', 'TAG'
    )
    
    # Populate with test data
    for i in range(1000):
        r.hset(f'doc:{i}', mapping={
            'title': f'Document {i}',
            'body': f'This is the body of document {i} with some searchable text',
            'category': f'cat{i % 10}'
        })
    
    # Wait for indexing to complete
    while True:
        info = r.execute_command('FT.INFO', 'testidx')
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict[b'indexing']) == 0:
            break
        time.sleep(0.1)


def run_benchmark():
    """Run the query cache benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=False)
    
    print("Setting up test index...")
    setup_index(r)
    
    # Test query
    query = ['FT.SEARCH', 'testidx', 'document', 'LIMIT', '0', '10']
    
    # Benchmark with cache disabled
    print("\n=== Cache Disabled ===")
    r.execute_command('FT.CONFIG', 'SET', 'QUERY_CACHE_SIZE', '0')
    
    def search_no_cache():
        r.execute_command(*query)
    
    mean_no_cache = benchmark("Throughput (cache disabled)", search_no_cache, iterations=1000)
    
    # Benchmark with cache enabled
    print("\n=== Cache Enabled ===")
    r.execute_command('FT.CONFIG', 'SET', 'QUERY_CACHE_SIZE', '1000')
    
    # Warm up cache
    for _ in range(10):
        r.execute_command(*query)
    
    def search_with_cache():
        r.execute_command(*query)
    
    mean_with_cache = benchmark("Throughput (cache enabled)", search_with_cache, iterations=1000)
    
    # Calculate improvement
    improvement = mean_no_cache / mean_with_cache
    
    print("\n=== Results ===")
    print(f"Cache disabled: {mean_no_cache:.3f}ms")
    print(f"Cache enabled:  {mean_with_cache:.3f}ms")
    print(f"Improvement:    {improvement:.2f}x")
    print(f"Target:         10.00x")
    
    # Pass/fail
    if improvement >= 10.0:
        print("\n✓ PASS: Achieved target improvement")
        return 0
    else:
        print(f"\n✗ FAIL: Did not achieve target (got {improvement:.2f}x, need 10.00x)")
        return 1


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

