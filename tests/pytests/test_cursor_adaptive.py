from common import *
from time import sleep, time

def loadDocs(env, count=100, idx='idx', text='hello world'):
    """Helper to create an index and load documents"""
    env.expect('FT.CREATE', idx, 'ON', 'HASH', 'prefix', 1, idx, 'SCHEMA', 'f1', 'TEXT').ok()
    waitForIndex(env, idx)
    con = env.getClusterConnectionIfNeeded()
    for x in range(count):
        cmd = ['FT.ADD', idx, f'{idx}_doc{x}', 1.0, 'FIELDS', 'f1', text]
        con.execute_command(*cmd)
    r1 = env.cmd('ft.search', idx, text)
    r2 = list(set(map(lambda x: x[1], filter(lambda x: isinstance(x, list), r1))))
    env.assertEqual([text], r2)
    r3 = to_dict(env.cmd('ft.info', idx))
    env.assertEqual(count, int(r3['num_docs']))

def exhaustCursor(env, idx, res, *args):
    """Helper to exhaust a cursor and return all rows"""
    first, cid = res
    rows = [res]
    while cid:
        res, cid = env.cmd('FT.CURSOR', 'READ', idx, cid, *args)
        rows.append([res, cid])
    return rows

    
def test_adaptive_option_accepted(env):
    """Test that ADAPTIVE option is accepted in WITHCURSOR"""
    loadDocs(env, count=100)
    
    # Test ADAPTIVE without COUNT
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 'WITHCURSOR', 'ADAPTIVE')
    env.assertNotEqual(cid, 0)
    env.cmd('FT.CURSOR', 'DEL', 'idx', cid)
    
    # Test ADAPTIVE with COUNT
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)
    env.assertNotEqual(cid, 0)
    env.cmd('FT.CURSOR', 'DEL', 'idx', cid)
    
    # Test COUNT with ADAPTIVE (order shouldn't matter)
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 'WITHCURSOR', 'COUNT', 10, 'ADAPTIVE')
    env.assertNotEqual(cid, 0)
    env.cmd('FT.CURSOR', 'DEL', 'idx', cid)
    
def test_adaptive_with_maxidle(env):
    """Test that ADAPTIVE works with MAXIDLE"""
    loadDocs(env, count=100)
    
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10, 'MAXIDLE', 5000)
    env.assertNotEqual(cid, 0)
    env.cmd('FT.CURSOR', 'DEL', 'idx', cid)
    
def test_adaptive_initial_chunk_uses_count(env):
    """Test that the initial chunk respects the COUNT parameter"""
    loadDocs(env, count=100)
    
    # Request 10 results in first chunk
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)
    
    # First response should have 10 results + 1 for count field
    env.assertEqual(len(res), 11)
    env.assertNotEqual(cid, 0)
    
    # Clean up
    env.cmd('FT.CURSOR', 'DEL', 'idx', cid)
    
def test_adaptive_completes_successfully(env):
    """Test that adaptive cursors can be exhausted successfully"""
    loadDocs(env, count=100)
    
    res = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 
                      'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)
    rows = exhaustCursor(env, 'idx', res)
    
    # Should have received all 100 results
    total_results = sum(len(row[0]) - 1 for row in rows)  # -1 for count field
    env.assertEqual(total_results, 100)
    
    # Last cursor should be 0 (depleted)
    env.assertEqual(rows[-1][1], 0)
    
def test_non_adaptive_unchanged(env):
    """Test that non-ADAPTIVE cursors maintain constant chunk size"""
    loadDocs(env, count=100)
    
    # Create cursor without ADAPTIVE
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1', 
                            'WITHCURSOR', 'COUNT', 10)
    
    chunk_sizes = []
    chunk_sizes.append(len(res) - 1)  # -1 for count field
    
    # Read several chunks
    for _ in range(5):
        if cid == 0:
            break
        res, cid = env.cmd('FT.CURSOR', 'READ', 'idx', cid)
        if len(res) > 1:  # Skip empty results
            chunk_sizes.append(len(res) - 1)
    
    # All non-empty chunks should be the same size (10) except possibly the last
    for i, size in enumerate(chunk_sizes[:-1]):
        env.assertEqual(size, 10, 
            message=f"Chunk {i} has size {size}, expected 10")
    
    # Clean up if cursor still exists
    if cid != 0:
        env.cmd('FT.CURSOR', 'DEL', 'idx', cid)

def test_adaptive_with_large_dataset(env):
    """Test adaptive behavior with a larger dataset"""
    # Create a larger dataset to allow for multiple chunk adjustments
    loadDocs(env, count=1000)

    # Start with a small chunk size
    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*', 'LOAD', 1, '@f1',
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 5)

    env.assertNotEqual(cid, 0)

    # Read through the cursor
    iterations = 0
    while cid != 0 and iterations < 100:  # Safety limit
        res, cid = env.cmd('FT.CURSOR', 'READ', 'idx', cid)
        iterations += 1

    # Should have completed successfully
    env.assertEqual(cid, 0)

def test_adaptive_with_sortby(env):
    """Test that ADAPTIVE works with SORTBY"""
    loadDocs(env, count=100)

    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*',
                            'LOAD', 1, '@f1',
                            'SORTBY', 1, '@f1',
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)

    env.assertNotEqual(cid, 0)

    # Exhaust the cursor
    rows = exhaustCursor(env, 'idx', [res, cid])

    # Should have received all results
    total_results = sum(len(row[0]) - 1 for row in rows)
    env.assertEqual(total_results, 100)

def test_adaptive_with_groupby(env):
    """Test that ADAPTIVE works with GROUPBY"""
    loadDocs(env, count=100)

    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*',
                            'GROUPBY', 1, '@f1',
                            'REDUCE', 'COUNT', 0, 'AS', 'count',
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 5)

    env.assertNotEqual(cid, 0)

    # Should be able to read the results
    rows = exhaustCursor(env, 'idx', [res, cid])

    # Should have at least one result (all docs have same f1 value)
    total_results = sum(len(row[0]) - 1 for row in rows)
    env.assertGreater(total_results, 0)

def test_adaptive_empty_results(env):
    """Test ADAPTIVE with a query that returns no results"""
    loadDocs(env, count=100)

    # Query that matches nothing
    res, cid = env.cmd('FT.AGGREGATE', 'idx', 'nonexistent',
                            'LOAD', 1, '@f1',
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)

    # Should return empty results and cursor should be 0
    env.assertEqual(len(res), 1)  # Just the count field
    env.assertEqual(cid, 0)

def test_adaptive_single_result(env):
    """Test ADAPTIVE with a query that returns a single result"""
    loadDocs(env, count=1)

    res, cid = env.cmd('FT.AGGREGATE', 'idx', '*',
                            'LOAD', 1, '@f1',
                            'WITHCURSOR', 'ADAPTIVE', 'COUNT', 10)

    # Should return 1 result and cursor should be 0 (depleted)
    env.assertEqual(len(res), 2)  # count field + 1 result
    env.assertEqual(cid, 0)

