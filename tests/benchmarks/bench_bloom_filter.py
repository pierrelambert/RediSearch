#!/usr/bin/env python3
"""
Bloom Filter Benchmark

Measures latency reduction for queries on non-existent terms.
Target: 50% reduction vs without bloom filter.
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


def setup_index(r, use_bloom=True):
    """Create test index and populate with data."""
    # Drop index if exists
    try:
        r.execute_command('FT.DROPINDEX', 'testidx', 'DD')
    except redis.ResponseError:
        pass
    
    # Create index with or without bloom filter
    cmd = [
        'FT.CREATE', 'testidx',
        'ON', 'HASH',
        'PREFIX', '1', 'doc:',
        'SCHEMA',
        'title', 'TEXT'
    ]
    
    if not use_bloom:
        cmd.insert(2, 'NOOFFSETS')
        cmd.insert(2, 'NOFREQS')
    
    r.execute_command(*cmd)
    
    # Populate with test data (large dataset to make bloom filter effective)
    for i in range(10000):
        r.hset(f'doc:{i}', mapping={
            'title': f'document number {i} with common words'
        })
    
    # Wait for indexing to complete
    while True:
        info = r.execute_command('FT.INFO', 'testidx')
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict[b'indexing']) == 0:
            break
        time.sleep(0.1)


def run_benchmark():
    """Run the bloom filter benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=False)
    
    # Non-existent terms to search for
    non_existent_terms = [
        'xyzabc123',
        'qwerty999',
        'nonexistent',
        'notfound456',
        'missing789'
    ]
    
    # Benchmark without bloom filter
    print("Setting up index without bloom filter...")
    setup_index(r, use_bloom=False)
    
    print("\n=== Without Bloom Filter ===")
    
    def search_no_bloom():
        for term in non_existent_terms:
            r.execute_command('FT.SEARCH', 'testidx', term, 'LIMIT', '0', '10')
    
    mean_no_bloom = benchmark("Latency (no bloom filter)", search_no_bloom, iterations=200)
    
    # Benchmark with bloom filter
    print("\nSetting up index with bloom filter...")
    setup_index(r, use_bloom=True)
    
    print("\n=== With Bloom Filter ===")
    
    def search_with_bloom():
        for term in non_existent_terms:
            r.execute_command('FT.SEARCH', 'testidx', term, 'LIMIT', '0', '10')
    
    mean_with_bloom = benchmark("Latency (with bloom filter)", search_with_bloom, iterations=200)
    
    # Calculate reduction
    reduction = (mean_no_bloom - mean_with_bloom) / mean_no_bloom * 100
    
    print("\n=== Results ===")
    print(f"Without bloom filter: {mean_no_bloom:.3f}ms")
    print(f"With bloom filter:    {mean_with_bloom:.3f}ms")
    print(f"Reduction:            {reduction:.1f}%")
    print(f"Target:               50.0%")
    
    # Pass/fail
    if reduction >= 50.0:
        print("\n✓ PASS: Achieved target reduction")
        return 0
    else:
        print(f"\n✗ FAIL: Did not achieve target (got {reduction:.1f}%, need 50.0%)")
        return 1


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

