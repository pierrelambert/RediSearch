from common import *


def set_bloom_filter(env, enabled):
    env.expect(config_cmd(), 'SET', 'BLOOM_FILTER_ENABLED', 'true' if enabled else 'false').ok()


def test_bloom_filter_info_and_toggle(env):
    conn = getConnectionByEnv(env)

    try:
        set_bloom_filter(env, True)
        env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

        for i in range(32):
            conn.execute_command('HSET', f'doc:{i}', 'text', f'bfterm{i}')

        waitForIndex(env, 'idx')

        info = index_info(env, 'idx')
        env.assertGreater(int(info['bloom_capacity']), 0)
        env.assertGreater(int(info['bloom_items']), 0)
        env.assertGreater(float(info['bloom_fpr']), 0)
        env.assertGreater(int(info['bloom_memory_bytes']), 0)

        set_bloom_filter(env, False)
        info = index_info(env, 'idx')
        env.assertEqual(int(info['bloom_capacity']), 0)
        env.assertEqual(int(info['bloom_items']), 0)
        env.assertEqual(float(info['bloom_fpr']), 0)
        env.assertEqual(int(info['bloom_memory_bytes']), 0)

        env.expect('FT.SEARCH', 'idx', '@text:bfterm7').equal([1, 'doc:7', ['text', 'bfterm7']])
        env.expect('FT.SEARCH', 'idx', '@text:not_present').equal([0])

        set_bloom_filter(env, True)
        info = index_info(env, 'idx')
        env.assertGreaterEqual(int(info['bloom_items']), 32)
        env.assertGreaterEqual(int(info['bloom_capacity']), 10000)
    finally:
        set_bloom_filter(env, True)


def test_bloom_filter_rebuilds_when_capacity_threshold_is_crossed(env):
    conn = getConnectionByEnv(env)

    try:
        set_bloom_filter(env, True)
        env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

        terms = ' '.join(f'zzterm{i:05d}' for i in range(8205))
        conn.execute_command('HSET', 'doc:bulk', 'text', terms)
        waitForIndex(env, 'idx')

        info = index_info(env, 'idx')
        env.assertGreaterEqual(int(info['bloom_capacity']), 20000)
        env.assertGreaterEqual(int(info['bloom_items']), 8205)
        env.assertLess(float(info['bloom_fpr']), 0.02)

        result = env.cmd('FT.SEARCH', 'idx', '@text:zzterm08204', 'LIMIT', '0', '1')
        env.assertEqual(result[0], 1)
        env.assertEqual(result[1], 'doc:bulk')
        env.expect('FT.SEARCH', 'idx', '@text:zzterm99999').equal([0])
    finally:
        set_bloom_filter(env, True)


def test_bloom_filter_is_correct_with_toggle_on_and_off(env):
    conn = getConnectionByEnv(env)

    try:
        set_bloom_filter(env, True)
        env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

        docs = {
            'doc:1': 'alpha beta',
            'doc:2': 'beta gamma',
            'doc:3': 'gamma delta',
        }
        for key, value in docs.items():
            conn.execute_command('HSET', key, 'text', value)

        waitForIndex(env, 'idx')

        enabled_hits = env.cmd('FT.SEARCH', 'idx', '@text:beta')
        enabled_miss = env.cmd('FT.SEARCH', 'idx', '@text:missingterm')

        set_bloom_filter(env, False)
        disabled_hits = env.cmd('FT.SEARCH', 'idx', '@text:beta')
        disabled_miss = env.cmd('FT.SEARCH', 'idx', '@text:missingterm')

        env.assertEqual(disabled_hits, enabled_hits)
        env.assertEqual(disabled_miss, enabled_miss)
    finally:
        set_bloom_filter(env, True)

