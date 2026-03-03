from common import *
from time import sleep

"""
Comprehensive tests for the Query Cache feature.

The query cache stores query results keyed by (query_hash, index_revision).
Cache is invalidated when documents are added/updated/deleted (revision bump).
"""

def test_cache_hit_identical_query(env):
    """Test that identical queries hit the cache on second run"""
    conn = getConnectionByEnv(env)
    
    # Create index and add documents
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT', 'year', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'title', 'hello world', 'year', '2020')
    conn.execute_command('HSET', 'doc2', 'title', 'hello redis', 'year', '2021')
    conn.execute_command('HSET', 'doc3', 'title', 'world news', 'year', '2022')
    
    # First query - should miss cache
    res1 = env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '10')
    info1 = index_info(env, 'idx')
    
    # Verify cache stats exist and show 1 miss
    env.assertIn('query_cache_stats', info1)
    cache_stats1 = info1['query_cache_stats']
    env.assertEqual(cache_stats1['misses'], 1)
    env.assertEqual(cache_stats1['hits'], 0)
    
    # Second identical query - should hit cache
    res2 = env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '10')
    info2 = index_info(env, 'idx')
    cache_stats2 = info2['query_cache_stats']
    
    # Verify cache hit
    env.assertEqual(cache_stats2['hits'], 1)
    env.assertEqual(cache_stats2['misses'], 1)
    
    # Results should be identical
    env.assertEqual(res1, res2)

def test_cache_miss_different_query(env):
    """Test that different queries don't hit each other's cache"""
    env = env
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello world')
    conn.execute_command('HSET', 'doc2', 'title', 'goodbye world')
    
    # Query 1
    env.cmd('FT.SEARCH', 'idx', 'hello')
    info1 = index_info(env, 'idx')
    env.assertEqual(info1['query_cache_stats']['misses'], 1)
    
    # Query 2 (different query string)
    env.cmd('FT.SEARCH', 'idx', 'goodbye')
    info2 = index_info(env, 'idx')
    
    # Should have 2 misses (both queries missed)
    env.assertEqual(info2['query_cache_stats']['misses'], 2)
    env.assertEqual(info2['query_cache_stats']['hits'], 0)

def test_cache_invalidation_on_add(env):
    """Test cache invalidation when documents are added"""
    env = env
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')
    
    # First query - cache miss
    res1 = env.cmd('FT.SEARCH', 'idx', 'hello')
    env.assertEqual(res1[0], 1)  # 1 result
    
    # Second query - cache hit
    env.cmd('FT.SEARCH', 'idx', 'hello')
    info = index_info(env, 'idx')
    env.assertEqual(info['query_cache_stats']['hits'], 1)
    
    # Add new document - bumps revision
    conn.execute_command('HSET', 'doc2', 'title', 'hello world')
    
    # Third query - should miss due to revision change
    res3 = env.cmd('FT.SEARCH', 'idx', 'hello')
    env.assertEqual(res3[0], 2)  # 2 results now
    info3 = index_info(env, 'idx')
    
    # Should have 2 misses (initial + after add)
    env.assertEqual(info3['query_cache_stats']['misses'], 2)
    env.assertEqual(info3['query_cache_stats']['hits'], 1)

def test_cache_invalidation_on_update(env):
    """Test cache invalidation when documents are updated"""
    env = env
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')
    
    # Query and cache
    env.cmd('FT.SEARCH', 'idx', 'hello')
    env.cmd('FT.SEARCH', 'idx', 'hello')  # Hit cache
    
    # Update document
    conn.execute_command('HSET', 'doc1', 'title', 'goodbye')
    
    # Query again - should miss
    env.cmd('FT.SEARCH', 'idx', 'hello')
    info = index_info(env, 'idx')
    
    # 2 misses (before and after update), 1 hit (second query)
    env.assertEqual(info['query_cache_stats']['misses'], 2)
    env.assertEqual(info['query_cache_stats']['hits'], 1)

def test_cache_invalidation_on_delete(env):
    """Test cache invalidation when documents are deleted"""
    env = env
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')
    conn.execute_command('HSET', 'doc2', 'title', 'hello world')
    
    # Query and cache
    res1 = env.cmd('FT.SEARCH', 'idx', 'hello')
    env.assertEqual(res1[0], 2)
    env.cmd('FT.SEARCH', 'idx', 'hello')  # Hit cache
    
    # Delete document
    conn.execute_command('DEL', 'doc1')
    
    # Query again - should miss and return different results
    res3 = env.cmd('FT.SEARCH', 'idx', 'hello')
    env.assertEqual(res3[0], 1)
    info = index_info(env, 'idx')
    
    env.assertEqual(info['query_cache_stats']['misses'], 2)
    env.assertEqual(info['query_cache_stats']['hits'], 1)

@skip(cluster=True)
def test_cache_config_max_size(env):
    """Test FT.CONFIG SET QUERYCACHE_MAX_SIZE"""
    env = env

    # Check default value
    config = env.cmd(config_cmd(), 'GET', 'QUERYCACHE_MAX_SIZE')
    env.assertEqual(config[0][0], 'QUERYCACHE_MAX_SIZE')
    default_size = int(config[0][1])
    env.assertGreater(default_size, 0)

    # Set to new value
    env.cmd(config_cmd(), 'SET', 'QUERYCACHE_MAX_SIZE', '500')
    config2 = env.cmd(config_cmd(), 'GET', 'QUERYCACHE_MAX_SIZE')
    env.assertEqual(config2[0][1], '500')

    # Set to 0 (disabled)
    env.cmd(config_cmd(), 'SET', 'QUERYCACHE_MAX_SIZE', '0')
    config3 = env.cmd(config_cmd(), 'GET', 'QUERYCACHE_MAX_SIZE')
    env.assertEqual(config3[0][1], '0')

@skip(cluster=True)
def test_cache_disabled_when_size_zero(env):
    """Test that cache is disabled when QUERYCACHE_MAX_SIZE is 0"""
    env = env
    conn = getConnectionByEnv(env)

    # Disable cache
    env.cmd(config_cmd(), 'SET', 'QUERYCACHE_MAX_SIZE', '0')

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')

    # Run query twice
    env.cmd('FT.SEARCH', 'idx', 'hello')
    env.cmd('FT.SEARCH', 'idx', 'hello')

    # Cache stats should not appear in FT.INFO when disabled
    info = index_info(env, 'idx')
    self.assertNotIn('query_cache_stats', info)

def test_cursor_queries_not_cached(env):
    """Test that WITHCURSOR queries are not cached"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    for i in range(20):
        conn.execute_command('HSET', f'doc{i}', 'title', 'hello')

    # Run cursor query twice
    res1 = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', '1', '@title',
                   'WITHCURSOR', 'COUNT', '5')
    cursor_id1 = res1[1]

    res2 = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', '1', '@title',
                   'WITHCURSOR', 'COUNT', '5')
    cursor_id2 = res2[1]

    # Clean up cursors
    if cursor_id1 != 0:
        env.cmd('FT.CURSOR', 'DEL', 'idx', cursor_id1)
    if cursor_id2 != 0:
        env.cmd('FT.CURSOR', 'DEL', 'idx', cursor_id2)

    # Check cache stats - should have 0 hits (cursor queries not cached)
    info = index_info(env, 'idx')
    if 'query_cache_stats' in info:
        # If stats exist, hits should be 0
        env.assertEqual(info['query_cache_stats']['hits'], 0)

def test_unlimited_queries_not_cached(env):
    """Test that unlimited queries (LIMIT 0 0) are not cached"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')

    # Run unlimited query twice (LIMIT 0 0 means count only)
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '0')
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '0')

    # Check cache stats - unlimited queries should not be cached
    info = index_info(env, 'idx')
    if 'query_cache_stats' in info:
        # If stats exist, hits should be 0
        env.assertEqual(info['query_cache_stats']['hits'], 0)

def test_different_limit_different_cache_key(env):
    """Test that different LIMIT values create different cache keys"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    for i in range(10):
        conn.execute_command('HSET', f'doc{i}', 'title', 'hello')

    # Query with LIMIT 5
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '5')
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '5')  # Should hit

    # Query with LIMIT 3 (different limit)
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '3')  # Should miss

    info = index_info(env, 'idx')
    cache_stats = info['query_cache_stats']

    # Should have 2 misses (LIMIT 5 first time, LIMIT 3 first time)
    # and 1 hit (LIMIT 5 second time)
    env.assertEqual(cache_stats['misses'], 2)
    env.assertEqual(cache_stats['hits'], 1)

def test_different_offset_different_cache_key(env):
    """Test that different OFFSET values create different cache keys"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    for i in range(10):
        conn.execute_command('HSET', f'doc{i}', 'title', 'hello')

    # Query with OFFSET 0
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '5')
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '0', '5')  # Should hit

    # Query with OFFSET 2 (different offset)
    env.cmd('FT.SEARCH', 'idx', 'hello', 'LIMIT', '2', '5')  # Should miss

    info = index_info(env, 'idx')
    cache_stats = info['query_cache_stats']

    # Should have 2 misses and 1 hit
    env.assertEqual(cache_stats['misses'], 2)
    env.assertEqual(cache_stats['hits'], 1)

def test_cache_stats_verification(env):
    """Test comprehensive cache statistics"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'hello')

    # Run multiple queries to build up stats
    env.cmd('FT.SEARCH', 'idx', 'hello')  # Miss
    env.cmd('FT.SEARCH', 'idx', 'hello')  # Hit
    env.cmd('FT.SEARCH', 'idx', 'hello')  # Hit
    env.cmd('FT.SEARCH', 'idx', 'world')  # Miss (different query)

    info = index_info(env, 'idx')
    cache_stats = info['query_cache_stats']

    # Verify all stats fields exist
    env.assertIn('hits', cache_stats)
    env.assertIn('misses', cache_stats)
    env.assertIn('hit_rate', cache_stats)
    env.assertIn('entries', cache_stats)
    env.assertIn('memory_bytes', cache_stats)
    env.assertIn('evictions', cache_stats)

    # Verify values
    env.assertEqual(cache_stats['hits'], 2)
    env.assertEqual(cache_stats['misses'], 2)

    # Hit rate should be 50% (2 hits out of 4 lookups)
    expected_hit_rate = 2.0 / 4.0
    self.assertAlmostEqual(cache_stats['hit_rate'], expected_hit_rate, places=2)

    # Should have 2 entries (one for each unique query)
    env.assertEqual(cache_stats['entries'], 2)

    # Memory should be > 0
    env.assertGreater(cache_stats['memory_bytes'], 0)

    # No evictions yet
    env.assertEqual(cache_stats['evictions'], 0)

def test_cache_hit_rate_calculation(env):
    """Test hit rate calculation with various scenarios"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'title', 'test')

    # All hits scenario (after first miss)
    env.cmd('FT.SEARCH', 'idx', 'test')  # Miss
    for _ in range(9):
        env.cmd('FT.SEARCH', 'idx', 'test')  # Hit

    info = index_info(env, 'idx')
    cache_stats = info['query_cache_stats']

    # 9 hits, 1 miss = 90% hit rate
    env.assertEqual(cache_stats['hits'], 9)
    env.assertEqual(cache_stats['misses'], 1)
    self.assertAlmostEqual(cache_stats['hit_rate'], 0.9, places=2)

def test_cache_with_aggregate_query(env):
    """Test that FT.AGGREGATE queries are also cached"""
    env = env
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT', 'year', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'title', 'hello', 'year', '2020')
    conn.execute_command('HSET', 'doc2', 'title', 'world', 'year', '2021')

    # Run aggregate query twice
    res1 = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', '1', '@title')
    res2 = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', '1', '@title')

    info = index_info(env, 'idx')
    cache_stats = info['query_cache_stats']

    # Should have 1 hit (second query)
    env.assertEqual(cache_stats['hits'], 1)
    env.assertEqual(cache_stats['misses'], 1)

    # Results should be identical
    env.assertEqual(res1, res2)

def test_cache_multiple_indexes(env):
    """Test that cache works correctly with multiple indexes"""
    env = env
    conn = getConnectionByEnv(env)

    # Create two indexes
    env.cmd('FT.CREATE', 'idx1', 'SCHEMA', 'title', 'TEXT')
    env.cmd('FT.CREATE', 'idx2', 'SCHEMA', 'title', 'TEXT')

    conn.execute_command('HSET', 'doc1', 'title', 'hello')
    conn.execute_command('HSET', 'doc2', 'title', 'hello')

    # Query both indexes with same query string
    env.cmd('FT.SEARCH', 'idx1', 'hello')
    env.cmd('FT.SEARCH', 'idx1', 'hello')  # Hit for idx1
    env.cmd('FT.SEARCH', 'idx2', 'hello')  # Miss for idx2 (different index)
    env.cmd('FT.SEARCH', 'idx2', 'hello')  # Hit for idx2

    # Check stats for idx1
    info1 = index_info(env, 'idx1')
    cache_stats1 = info1['query_cache_stats']
    env.assertEqual(cache_stats1['hits'], 1)
    env.assertEqual(cache_stats1['misses'], 1)

    # Check stats for idx2
    info2 = index_info(env, 'idx2')
    cache_stats2 = info2['query_cache_stats']
    env.assertEqual(cache_stats2['hits'], 1)
    env.assertEqual(cache_stats2['misses'], 1)

