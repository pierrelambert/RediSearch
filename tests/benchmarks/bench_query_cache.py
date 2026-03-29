#!/usr/bin/env python3
"""Query Cache Benchmark using the real runtime cache toggles."""

import redis
import sys

from benchmark_utils import (
    assert_total_count,
    benchmark,
    drop_index_if_exists,
    ft_config_set_and_report,
    restore_configs,
    snapshot_configs,
    wait_for_indexing,
)


INDEX_NAME = 'testidx'
QUERY = ['FT.SEARCH', INDEX_NAME, 'document', 'LIMIT', '0', '10']
EXPECTED_TOTAL = 1000
CONFIG_NAMES = ['QUERYCACHE_MAX_SIZE', 'QUERYCACHE_ENABLED']


def setup_index(r):
    """Create test index and populate with data."""
    drop_index_if_exists(r, INDEX_NAME)
    
    # Create index
    r.execute_command(
        'FT.CREATE', INDEX_NAME,
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
    
    wait_for_indexing(r, INDEX_NAME)


def validate_query_result(r, label):
    """Confirm the benchmark query returns the expected result count."""
    result = r.execute_command(*QUERY)
    assert_total_count(result, EXPECTED_TOTAL, label)


def run_phase(r, label, enabled):
    """Run one query-cache benchmark phase."""
    print(f"\n=== {label} ===")
    ft_config_set_and_report(r, 'QUERYCACHE_MAX_SIZE', '1000')
    ft_config_set_and_report(r, 'QUERYCACHE_ENABLED', 'true' if enabled else 'false')
    validate_query_result(r, f'{label} sanity query')

    if enabled:
        print('Warming cache with repeated identical queries...')
        for _ in range(10):
            r.execute_command(*QUERY)

    return benchmark(
        f'Latency ({label.lower()})',
        lambda: r.execute_command(*QUERY),
        iterations=1000,
    )


def run_benchmark():
    """Run the query cache benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=True)
    original_configs = snapshot_configs(r, CONFIG_NAMES)

    try:
        print('Setting up test index...')
        setup_index(r)

        disabled_stats = run_phase(r, 'Cache Disabled', enabled=False)
        enabled_stats = run_phase(r, 'Cache Enabled', enabled=True)

        improvement = disabled_stats['mean_ms'] / enabled_stats['mean_ms']

        print('\n=== Results ===')
        print(f"Cache disabled mean: {disabled_stats['mean_ms']:.3f}ms")
        print(f"Cache enabled mean:  {enabled_stats['mean_ms']:.3f}ms")
        print(f"Improvement:         {improvement:.2f}x")
        return 0
    finally:
        restore_configs(r, original_configs)
        drop_index_if_exists(r, INDEX_NAME)


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

