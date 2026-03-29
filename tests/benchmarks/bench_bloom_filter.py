#!/usr/bin/env python3
"""Bloom filter benchmark using the real runtime bloom toggle."""

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
NON_EXISTENT_TERMS = ['xyzabc123', 'qwerty999', 'nonexistent', 'notfound456', 'missing789']
CONFIG_NAMES = ['BLOOM_FILTER_ENABLED']


def setup_index(r):
    """Create test index and populate with data."""
    drop_index_if_exists(r, INDEX_NAME)

    r.execute_command(
        'FT.CREATE', INDEX_NAME,
        'ON', 'HASH',
        'PREFIX', '1', 'doc:',
        'SCHEMA',
        'title', 'TEXT'
    )
    
    # Populate with test data (large dataset to make bloom filter effective)
    for i in range(10000):
        r.hset(f'doc:{i}', mapping={
            'title': f'document number {i} with common words'
        })
    
    wait_for_indexing(r, INDEX_NAME)


def validate_queries(r, label):
    """Confirm the index contains data and negative probes return zero hits."""
    positive = r.execute_command('FT.SEARCH', INDEX_NAME, 'common', 'LIMIT', '0', '1')
    assert_total_count(positive, 10000, f'{label} positive-control query')

    for term in NON_EXISTENT_TERMS:
        result = r.execute_command('FT.SEARCH', INDEX_NAME, term, 'LIMIT', '0', '10')
        assert_total_count(result, 0, f'{label} missing-term query {term}')


def run_phase(r, label, enabled):
    """Run one bloom-filter benchmark phase."""
    print(f"\n=== {label} ===")
    ft_config_set_and_report(r, 'BLOOM_FILTER_ENABLED', 'true' if enabled else 'false')
    validate_queries(r, label)

    return benchmark(
        f'Latency ({label.lower()})',
        lambda: [
            r.execute_command('FT.SEARCH', INDEX_NAME, term, 'LIMIT', '0', '10')
            for term in NON_EXISTENT_TERMS
        ],
        iterations=200,
    )


def run_benchmark():
    """Run the bloom filter benchmark."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=True)
    original_configs = snapshot_configs(r, CONFIG_NAMES)

    try:
        print('Setting up benchmark index...')
        setup_index(r)

        disabled_stats = run_phase(r, 'Bloom Disabled', enabled=False)
        enabled_stats = run_phase(r, 'Bloom Enabled', enabled=True)

        reduction = (
            (disabled_stats['mean_ms'] - enabled_stats['mean_ms'])
            / disabled_stats['mean_ms']
            * 100
        )

        print('\n=== Results ===')
        print(f"Bloom disabled mean: {disabled_stats['mean_ms']:.3f}ms")
        print(f"Bloom enabled mean:  {enabled_stats['mean_ms']:.3f}ms")
        print(f"Mean reduction:      {reduction:.1f}%")
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

