#!/usr/bin/env python3
"""
Numeric Tree Benchmark

Measures memory usage reduction with sparse numeric data.
Target: 30-50% reduction for sparse numeric distributions.
"""

import redis
import time
import sys


def get_memory_usage(r, index_name):
    """Get memory usage for numeric index using FT.DEBUG NUMIDX_SUMMARY."""
    try:
        result = r.execute_command('FT.DEBUG', 'NUMIDX_SUMMARY', index_name, 'numfield')
        # Parse the result to extract memory usage
        # Result format varies, but typically includes memory info
        result_str = str(result)
        # Look for memory-related metrics
        return result
    except redis.ResponseError as e:
        print(f"Warning: Could not get NUMIDX_SUMMARY: {e}")
        return None


def setup_dense_index(r):
    """Create index with dense numeric distribution."""
    # Drop index if exists
    try:
        r.execute_command('FT.DROPINDEX', 'denseidx', 'DD')
    except redis.ResponseError:
        pass
    
    # Create index
    r.execute_command(
        'FT.CREATE', 'denseidx',
        'ON', 'HASH',
        'PREFIX', '1', 'dense:',
        'SCHEMA',
        'numfield', 'NUMERIC'
    )
    
    # Populate with dense data (consecutive values)
    for i in range(10000):
        r.hset(f'dense:{i}', 'numfield', i)
    
    # Wait for indexing
    while True:
        info = r.execute_command('FT.INFO', 'denseidx')
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict[b'indexing']) == 0:
            break
        time.sleep(0.1)


def setup_sparse_index(r):
    """Create index with sparse numeric distribution."""
    # Drop index if exists
    try:
        r.execute_command('FT.DROPINDEX', 'sparseidx', 'DD')
    except redis.ResponseError:
        pass
    
    # Create index
    r.execute_command(
        'FT.CREATE', 'sparseidx',
        'ON', 'HASH',
        'PREFIX', '1', 'sparse:',
        'SCHEMA',
        'numfield', 'NUMERIC'
    )
    
    # Populate with sparse data (large gaps between values)
    for i in range(10000):
        # Use exponential distribution to create sparsity
        value = i * 1000 + (i % 100) * 10000
        r.hset(f'sparse:{i}', 'numfield', value)
    
    # Wait for indexing
    while True:
        info = r.execute_command('FT.INFO', 'sparseidx')
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict[b'indexing']) == 0:
            break
        time.sleep(0.1)


def get_index_memory(r, index_name):
    """Get total memory usage for an index from FT.INFO."""
    info = r.execute_command('FT.INFO', index_name)
    info_dict = dict(zip(info[::2], info[1::2]))
    
    # Try to get inverted index memory or total memory
    if b'inverted_sz_mb' in info_dict:
        return float(info_dict[b'inverted_sz_mb'])
    elif b'num_records' in info_dict:
        # Estimate based on number of records
        return int(info_dict[b'num_records']) * 0.001  # rough estimate
    
    return 0.0


def run_benchmark():
    """Run the numeric tree benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=False)
    
    print("Setting up dense numeric index...")
    setup_dense_index(r)
    dense_memory = get_index_memory(r, 'denseidx')
    
    print("Setting up sparse numeric index...")
    setup_sparse_index(r)
    sparse_memory = get_index_memory(r, 'sparseidx')
    
    print("\n=== Memory Usage ===")
    print(f"Dense index:  {dense_memory:.3f} MB")
    print(f"Sparse index: {sparse_memory:.3f} MB")
    
    # Get detailed numeric index info if available
    print("\n=== Numeric Index Details ===")
    dense_debug = get_memory_usage(r, 'denseidx')
    if dense_debug:
        print(f"Dense NUMIDX_SUMMARY: {dense_debug}")
    
    sparse_debug = get_memory_usage(r, 'sparseidx')
    if sparse_debug:
        print(f"Sparse NUMIDX_SUMMARY: {sparse_debug}")
    
    # Calculate reduction (sparse should use less memory due to tree optimization)
    if dense_memory > 0:
        reduction = (dense_memory - sparse_memory) / dense_memory * 100
    else:
        reduction = 0.0
    
    print("\n=== Results ===")
    print(f"Memory reduction: {reduction:.1f}%")
    print(f"Target range:     30.0% - 50.0%")
    
    # Pass/fail
    if 30.0 <= reduction <= 50.0:
        print("\n✓ PASS: Achieved target reduction")
        return 0
    else:
        print(f"\n✗ FAIL: Outside target range (got {reduction:.1f}%)")
        return 1


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

