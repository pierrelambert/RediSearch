#!/usr/bin/env python3
"""Parked benchmark for cursor adaptive default.

This benchmark is intentionally non-shipping. `WITHCURSOR ADAPTIVE` is a real
explicit query flag, but the truthful runtime toggle for this area is
`CURSOR_ADAPTIVE_DEFAULT`, and the current shipping request path does not read
that config. Measuring performance while flipping only the config would produce
placebo noise, so this script reports the config values and exits parked.
"""

import redis
import sys

from benchmark_utils import ft_config_set_and_report, restore_configs, snapshot_configs


CONFIG_NAMES = ['CURSOR_ADAPTIVE_DEFAULT']


def run_benchmark():
    """Report parked status instead of emitting placebo benchmark data."""
    r = redis.Redis(host='localhost', port=6379, decode_responses=True)
    original_configs = snapshot_configs(r, CONFIG_NAMES)

    try:
        print('=== Cursor Adaptive Benchmark: PARKED / NON-SHIPPING ===')
        ft_config_set_and_report(r, 'CURSOR_ADAPTIVE_DEFAULT', 'false')
        ft_config_set_and_report(r, 'CURSOR_ADAPTIVE_DEFAULT', 'true')
        print('\nReason for parking:')
        print('  - `WITHCURSOR ADAPTIVE` is a real explicit query flag.')
        print('  - `CURSOR_ADAPTIVE_DEFAULT` is the real FT.CONFIG toggle for default behavior.')
        print('  - Current audited request setup does not read `CURSOR_ADAPTIVE_DEFAULT`.')
        print('  - Benchmarking only the config would therefore measure placebo noise.')
        print('\nStatus: parked pending a config-backed execution path and measured justification.')
        return 0
    finally:
        restore_configs(r, original_configs)


if __name__ == '__main__':
    try:
        sys.exit(run_benchmark())
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

