#!/usr/bin/env python3
"""
Cursor Adaptive Benchmark

Measures average chunk processing time with ADAPTIVE cursor mode.
Target: Consistent ~10ms per chunk.
"""

import redis
import time
import statistics
import sys


def benchmark(name, func, iterations=100):
    """Run a benchmark and collect timing statistics."""
    times = []
    for _ in range(iterations):
        start = time.perf_counter()
        func()
        times.append(time.perf_counter() - start)
    
    mean = statistics.mean(times) * 1000
    median = statistics.median(times) * 1000
    p99 = sorted(times)[int(len(times) * 0.99)] * 1000
    stdev = statistics.stdev(times) * 1000 if len(times) > 1 else 0
    
    print(f"{name}:")
    print(f"  Mean:  {mean:.3f}ms")
    print(f"  P50:   {median:.3f}ms")
    print(f"  P99:   {p99:.3f}ms")
    print(f"  StdDev: {stdev:.3f}ms")
    
    return mean, stdev


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
        'value', 'NUMERIC'
    )
    
    # Populate with large dataset to ensure multiple cursor chunks
    for i in range(5000):
        r.hset(f'doc:{i}', mapping={
            'title': f'Document {i} with searchable content',
            'value': i
        })
    
    # Wait for indexing to complete
    while True:
        info = r.execute_command('FT.INFO', 'testidx')
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict[b'indexing']) == 0:
            break
        time.sleep(0.1)


def exhaust_cursor(r, index, cursor_id):
    """Read all chunks from a cursor and measure timing."""
    chunk_times = []
    
    while cursor_id != 0:
        start = time.perf_counter()
        result = r.execute_command('FT.CURSOR', 'READ', index, cursor_id)
        elapsed = (time.perf_counter() - start) * 1000
        
        chunk_times.append(elapsed)
        cursor_id = result[1]
    
    return chunk_times


def run_benchmark():
    """Run the cursor adaptive benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=False)
    
    print("Setting up test index...")
    setup_index(r)
    
    # Test with ADAPTIVE mode
    print("\n=== ADAPTIVE Cursor Mode ===")
    
    chunk_times_all = []
    
    for iteration in range(10):
        # Create cursor with ADAPTIVE mode
        result = r.execute_command(
            'FT.AGGREGATE', 'testidx', '*',
            'LOAD', '1', '@title',
            'WITHCURSOR',
            'COUNT', '100'
        )
        
        cursor_id = result[1]
        chunk_times = exhaust_cursor(r, 'testidx', cursor_id)
        chunk_times_all.extend(chunk_times)
    
    if not chunk_times_all:
        print("No chunks processed!")
        return 1
    
    mean = statistics.mean(chunk_times_all)
    median = statistics.median(chunk_times_all)
    stdev = statistics.stdev(chunk_times_all) if len(chunk_times_all) > 1 else 0
    p99 = sorted(chunk_times_all)[int(len(chunk_times_all) * 0.99)]
    
    print(f"Processed {len(chunk_times_all)} chunks")
    print(f"  Mean:   {mean:.3f}ms")
    print(f"  Median: {median:.3f}ms")
    print(f"  StdDev: {stdev:.3f}ms")
    print(f"  P99:    {p99:.3f}ms")
    
    # Check consistency (low standard deviation relative to mean)
    consistency = (stdev / mean * 100) if mean > 0 else 100
    
    print("\n=== Results ===")
    print(f"Average chunk time: {mean:.3f}ms")
    print(f"Target:             ~10.00ms")
    print(f"Consistency (CV):   {consistency:.1f}%")
    
    # Pass/fail: mean should be close to 10ms and consistent
    target_min = 5.0
    target_max = 15.0
    max_cv = 50.0  # Maximum coefficient of variation (%)
    
    if target_min <= mean <= target_max and consistency <= max_cv:
        print(f"\n✓ PASS: Achieved target (within {target_min}-{target_max}ms, CV < {max_cv}%)")
        return 0
    else:
        print(f"\n✗ FAIL: Outside target range or inconsistent")
        if not (target_min <= mean <= target_max):
            print(f"  Mean {mean:.3f}ms not in range {target_min}-{target_max}ms")
        if consistency > max_cv:
            print(f"  Consistency {consistency:.1f}% exceeds {max_cv}%")
        return 1


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

