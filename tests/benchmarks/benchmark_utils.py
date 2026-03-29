#!/usr/bin/env python3
"""Shared helpers for truthful benchmark scripts."""

import math
import time


def percentile_ms(samples, percentile):
    """Return a percentile in milliseconds using nearest-rank selection."""
    ordered = sorted(samples)
    rank = max(0, math.ceil((percentile / 100) * len(ordered)) - 1)
    return ordered[rank] * 1000


def benchmark(name, func, iterations):
    """Run a benchmark and print latency distribution statistics."""
    samples = []
    for _ in range(iterations):
        start = time.perf_counter()
        func()
        samples.append(time.perf_counter() - start)

    stats = {
        'mean_ms': (sum(samples) / len(samples)) * 1000,
        'p50_ms': percentile_ms(samples, 50),
        'p95_ms': percentile_ms(samples, 95),
        'p99_ms': percentile_ms(samples, 99),
    }

    print(f"{name}:")
    print(f"  Mean: {stats['mean_ms']:.3f}ms")
    print(f"  P50:  {stats['p50_ms']:.3f}ms")
    print(f"  P95:  {stats['p95_ms']:.3f}ms")
    print(f"  P99:  {stats['p99_ms']:.3f}ms")
    return stats


def drop_index_if_exists(r, index_name):
    """Drop an index if it already exists."""
    try:
        r.execute_command('FT.DROPINDEX', index_name, 'DD')
    except Exception:
        pass


def wait_for_indexing(r, index_name):
    """Wait until asynchronous indexing work completes."""
    while True:
        info = r.execute_command('FT.INFO', index_name)
        info_dict = dict(zip(info[::2], info[1::2]))
        if int(info_dict['indexing']) == 0:
            return
        time.sleep(0.1)


def ft_config_get(r, name):
    """Return the effective FT.CONFIG value as a string."""
    response = r.execute_command('FT.CONFIG', 'GET', name)
    if len(response) != 1 or len(response[0]) != 2:
        raise ValueError(f'Unexpected FT.CONFIG GET response for {name}: {response!r}')
    returned_name, value = response[0]
    if returned_name != name:
        raise ValueError(f'Expected config {name}, got {returned_name}')
    return value


def ft_config_set_and_report(r, name, value):
    """Set an FT.CONFIG value and print the effective value."""
    print(f"Setting FT.CONFIG {name} = {value}")
    r.execute_command('FT.CONFIG', 'SET', name, value)
    effective = ft_config_get(r, name)
    print(f"  Effective {name} = {effective}")
    return effective


def snapshot_configs(r, names):
    """Capture the current values for a set of FT.CONFIG names."""
    return {name: ft_config_get(r, name) for name in names}


def restore_configs(r, configs):
    """Restore FT.CONFIG values previously captured with snapshot_configs."""
    if not configs:
        return
    print("\nRestoring FT.CONFIG values...")
    for name, value in configs.items():
        ft_config_set_and_report(r, name, value)


def assert_total_count(result, expected_total, label):
    """Assert the total-count header of an FT.SEARCH/FT.AGGREGATE response."""
    actual_total = int(result[0])
    if actual_total != expected_total:
        raise AssertionError(
            f'{label} expected total {expected_total}, got {actual_total}: {result!r}'
        )
    print(f"Sanity check OK: {label} total={actual_total}")