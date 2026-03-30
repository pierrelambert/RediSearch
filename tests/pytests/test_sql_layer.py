# -*- coding: utf-8 -*-
#
# Copyright (c) 2006-Present, Redis Ltd.
# All rights reserved.
#
# Licensed under your choice of the Redis Source Available License 2.0
# (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
# GNU Affero General Public License v3 (AGPLv3).

"""
Comprehensive end-to-end tests for the SQL Semantic Layer.

Tests FT.SQL command functionality, verifying SQL to RQL translation
and result equivalence with FT.SEARCH/FT.AGGREGATE.
"""

import json

from common import *


def _sql_config_cmd(env):
    cached = getattr(env, '_sql_config_cmd_name', None)
    if cached is not None:
        return cached

    for command in ('_FT.CONFIG', 'FT.CONFIG'):
        try:
            env.cmd(command, 'GET', 'SQL_ENABLED')
            env._sql_config_cmd_name = command
            return command
        except redis_exceptions.ResponseError as error:
            if 'unknown command' in str(error).lower():
                continue
            raise

    raise AssertionError('Neither _FT.CONFIG nor FT.CONFIG is available in this test environment')


def _enable_sql(env):
    env.expect(_sql_config_cmd(env), 'SET', 'SQL_ENABLED', 'true').ok()


def _get_sql_connection(env):
    _enable_sql(env)
    return getConnectionByEnv(env)


def _get_total_results(response):
    if isinstance(response, dict):
        return response.get('total_results', 0)
    return response[0] if response else 0


def _get_results(response):
    if isinstance(response, dict):
        return response.get('results', [])
    return response[1:]


def _search_rows(response):
    if isinstance(response, dict):
        return [
            (result.get('id'), result.get('extra_attributes', {}))
            for result in _get_results(response)
        ]

    rows = []
    for i in range(1, len(response), 2):
        attributes = to_dict(response[i + 1]) if i + 1 < len(response) else {}
        rows.append((response[i], attributes))
    return rows


def _aggregate_rows(response):
    if isinstance(response, dict):
        return [row.get('extra_attributes', row) for row in _get_results(response)]
    return [to_dict(row) for row in _get_results(response)]


def _assert_search_parity(env, sql_result, native_result):
    env.assertEqual(_get_total_results(sql_result), _get_total_results(native_result))
    env.assertEqual(_search_rows(sql_result), _search_rows(native_result))


def _assert_aggregate_parity(env, sql_result, native_result):
    env.assertEqual(_get_total_results(sql_result), _get_total_results(native_result))
    env.assertEqual(_aggregate_rows(sql_result), _aggregate_rows(native_result))


def _assert_hybrid_parity(env, sql_response, hybrid_response):
    sql_results, sql_count = get_results_from_hybrid_response(sql_response)
    hybrid_results, hybrid_count = get_results_from_hybrid_response(hybrid_response)

    env.assertEqual(sql_count, hybrid_count)
    env.assertEqual(list(sql_results.keys()), list(hybrid_results.keys()))


def test_sql_disabled_by_default():
    env = Env(noDefaultModuleArgs=True)
    if env.env == 'existing-env':
        env.skip()

    env.expect(_sql_config_cmd(env), 'GET', 'SQL_ENABLED').equal([['SQL_ENABLED', 'false']])
    env.expect('FT.SQL', 'SELECT * FROM idx').error().contains('FT.SQL is disabled')
    env.stop()


def test_sql_runtime_disable_blocks_queries(env):
    _enable_sql(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    env.cmd('FT.SQL', "SELECT * FROM idx")

    env.expect(_sql_config_cmd(env), 'SET', 'SQL_ENABLED', 'false').ok()
    env.expect('FT.SQL', "SELECT * FROM idx").error().contains('FT.SQL is disabled')


# =============================================================================
# Basic SELECT Tests
# =============================================================================

def test_sql_select_star(env):
    """FT.SQL 'SELECT * FROM idx' should match FT.SEARCH idx '*'"""
    conn = _get_sql_connection(env)
    
    # Create index
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC')
    
    # Add test data
    conn.execute_command('HSET', 'doc1', 'name', 'Product A', 'price', '100')
    conn.execute_command('HSET', 'doc2', 'name', 'Product B', 'price', '200')
    
    # Test SQL query
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx")
    
    # Test equivalent RQL query
    rql_result = env.cmd('FT.SEARCH', 'idx', '*')
    
    # Verify results match (same doc count)
    env.assertEqual(sql_result[0], rql_result[0])


def test_sql_select_fields(env):
    """FT.SQL with SELECT field1, field2"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC', 'category', 'TAG')
    conn.execute_command('HSET', 'doc1', 'name', 'Test', 'price', '100', 'category', 'electronics')
    
    sql_result = env.cmd('FT.SQL', "SELECT name, price FROM idx")
    rql_result = env.cmd('FT.SEARCH', 'idx', '*', 'RETURN', '2', 'name', 'price')
    
    env.assertEqual(sql_result[0], rql_result[0])


# =============================================================================
# WHERE Clause Tests
# =============================================================================

def test_sql_where_equality(env):
    """FT.SQL with WHERE field = 'value' on TAG field"""
    conn = _get_sql_connection(env)

    # Use TAG field - SQL equality translates to RQL TAG syntax @field:{value}
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'status', 'TAG')
    conn.execute_command('HSET', 'doc1', 'status', 'active')
    conn.execute_command('HSET', 'doc2', 'status', 'inactive')

    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE status = 'active'")

    # Should find exactly the doc with 'active'
    env.assertEqual(sql_result[0], 1)


def test_sql_where_greater_than(env):
    """FT.SQL with WHERE field > value"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price > 75")
    
    env.assertEqual(sql_result[0], 2)  # doc2 and doc3


def test_sql_where_less_than(env):
    """FT.SQL with WHERE field < value"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price < 75")
    
    env.assertEqual(sql_result[0], 1)  # Only doc1


def test_sql_where_greater_equal(env):
    """FT.SQL with WHERE field >= value"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price >= 100")
    
    env.assertEqual(sql_result[0], 2)  # doc2 and doc3


def test_sql_where_less_equal(env):
    """FT.SQL with WHERE field <= value"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price <= 100")
    
    env.assertEqual(sql_result[0], 2)  # doc1 and doc2


def test_sql_where_between(env):
    """FT.SQL with WHERE field BETWEEN a AND b"""
    conn = _get_sql_connection(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price BETWEEN 75 AND 125")

    env.assertEqual(sql_result[0], 1)  # Only doc2 (price=100)


def test_sql_search_parity_with_colon_index_name(env):
    """FT.SQL should accept practical RediSearch index names such as idx:all."""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx:all', 'SCHEMA', 'category', 'TAG', 'price', 'NUMERIC', 'SORTABLE')
    conn.execute_command('HSET', 'doc1', 'category', 'electronics', 'price', '100')
    conn.execute_command('HSET', 'doc2', 'category', 'furniture', 'price', '200')
    waitForIndex(env, 'idx:all')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, price FROM idx:all WHERE category = 'electronics' ORDER BY price ASC LIMIT 1"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx:all', '@category:{electronics}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'category', 'price',
        'LIMIT', '0', '1',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_in_parity(env):
    """FT.SQL IN on TAG fields should match FT.SEARCH tag union semantics."""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'category', 'TAG', 'price', 'NUMERIC', 'SORTABLE')

    docs = [
        ('doc1', 'electronics', '100'),
        ('doc2', 'accessories', '150'),
        ('doc3', 'furniture', '200'),
    ]
    for doc_id, category, price in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, price FROM idx "
        "WHERE category IN ('electronics', 'accessories') ORDER BY price ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '@category:{electronics|accessories}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'category', 'price',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_not_in_parity(env):
    """FT.SQL NOT IN on TAG fields should match FT.SEARCH negated tag union semantics."""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'category', 'TAG', 'price', 'NUMERIC', 'SORTABLE')

    docs = [
        ('doc1', 'electronics', '100'),
        ('doc2', 'accessories', '150'),
        ('doc3', 'furniture', '200'),
        ('doc4', 'clearance', '250'),
    ]
    for doc_id, category, price in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, price FROM idx "
        "WHERE category NOT IN ('clearance', 'furniture') ORDER BY price ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '-@category:{clearance|furniture}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'category', 'price',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_like_parity(env):
    """FT.SQL LIKE should match FT.SEARCH wildcard syntax on wildcard-capable text fields."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'name', 'TEXT', 'WITHSUFFIXTRIE', 'SORTABLE',
        'category', 'TAG'
    )

    for doc_id, name, category in (
        ('doc1', 'Laptop', 'portable'),
        ('doc2', 'Desktop', 'workstation'),
        ('doc3', 'Phone', 'mobile'),
    ):
        conn.execute_command('HSET', doc_id, 'name', name, 'category', category)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT name FROM idx WHERE name LIKE '%top' ORDER BY name ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '@name:*top',
        'SORTBY', 'name', 'ASC',
        'RETURN', '1', 'name',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_not_like_parity(env):
    """FT.SQL NOT LIKE should match FT.SEARCH negated wildcard syntax."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'name', 'TEXT', 'WITHSUFFIXTRIE', 'SORTABLE'
    )

    for doc_id, name in (
        ('doc1', 'Laptop'),
        ('doc2', 'Desktop'),
        ('doc3', 'Phone'),
    ):
        conn.execute_command('HSET', doc_id, 'name', name)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT name FROM idx WHERE name NOT LIKE '%top' ORDER BY name ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '-@name:*top',
        'SORTBY', 'name', 'ASC',
        'RETURN', '1', 'name',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_is_null_parity(env):
    """FT.SQL IS NULL should match FT.SEARCH ismissing() on INDEXMISSING fields."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'title', 'TEXT', 'SORTABLE',
        'nickname', 'TAG', 'INDEXMISSING',
        'rank', 'NUMERIC', 'SORTABLE'
    )

    docs = [
        ('doc1', 'Alpha', 'ace', '1'),
        ('doc2', 'Bravo', None, '2'),
        ('doc3', 'Charlie', None, '3'),
    ]
    for doc_id, title, nickname, rank in docs:
        args = ['HSET', doc_id, 'title', title, 'rank', rank]
        if nickname is not None:
            args.extend(['nickname', nickname])
        conn.execute_command(*args)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT title, rank FROM idx WHERE nickname IS NULL ORDER BY rank ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', 'ismissing(@nickname)',
        'SORTBY', 'rank', 'ASC',
        'RETURN', '2', 'title', 'rank',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_is_not_null_parity(env):
    """FT.SQL IS NOT NULL should match FT.SEARCH negated ismissing() on INDEXMISSING fields."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'title', 'TEXT', 'SORTABLE',
        'nickname', 'TAG', 'INDEXMISSING',
        'rank', 'NUMERIC', 'SORTABLE'
    )

    docs = [
        ('doc1', 'Alpha', 'ace', '1'),
        ('doc2', 'Bravo', None, '2'),
        ('doc3', 'Charlie', 'captain', '3'),
    ]
    for doc_id, title, nickname, rank in docs:
        args = ['HSET', doc_id, 'title', title, 'rank', rank]
        if nickname is not None:
            args.extend(['nickname', nickname])
        conn.execute_command(*args)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT title, rank FROM idx WHERE nickname IS NOT NULL ORDER BY rank ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '-ismissing(@nickname)',
        'SORTBY', 'rank', 'ASC',
        'RETURN', '2', 'title', 'rank',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_boolean_composition_parity(env):
    """FT.SQL AND/OR/NOT composition should match equivalent FT.SEARCH boolean syntax."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'category', 'TAG',
        'status', 'TAG',
        'price', 'NUMERIC', 'SORTABLE'
    )

    docs = [
        ('doc1', 'electronics', 'active', '100'),
        ('doc2', 'accessories', 'active', '50'),
        ('doc3', 'electronics', 'archived', '200'),
        ('doc4', 'furniture', 'active', '300'),
    ]
    for doc_id, category, status, price in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'status', status, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, price FROM idx "
        "WHERE (category = 'electronics' OR category = 'accessories') "
        "AND NOT (status = 'archived') ORDER BY price ASC LIMIT 10"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx',
        '((@category:{electronics}) | (@category:{accessories})) (-(@status:{archived}))',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'category', 'price',
        'LIMIT', '0', '10',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


# =============================================================================
# ORDER BY Tests
# =============================================================================

def test_sql_order_by_asc(env):
    """FT.SQL with ORDER BY field ASC"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'SORTABLE', 'price', 'NUMERIC', 'SORTABLE')
    conn.execute_command('HSET', 'doc1', 'name', 'A', 'price', '100')
    conn.execute_command('HSET', 'doc2', 'name', 'B', 'price', '50')
    conn.execute_command('HSET', 'doc3', 'name', 'C', 'price', '150')

    # Order by price ascending
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx ORDER BY price ASC")
    rql_result = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'price', 'ASC')

    # First result should be same (doc2 with price 50)
    env.assertEqual(sql_result[1], rql_result[1])


def test_sql_order_by_desc(env):
    """FT.SQL with ORDER BY field DESC"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'SORTABLE', 'price', 'NUMERIC', 'SORTABLE')
    conn.execute_command('HSET', 'doc1', 'name', 'A', 'price', '100')
    conn.execute_command('HSET', 'doc2', 'name', 'B', 'price', '50')
    conn.execute_command('HSET', 'doc3', 'name', 'C', 'price', '150')

    # Order by price descending
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx ORDER BY price DESC")
    rql_result = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'price', 'DESC')

    # First result should be same (doc3 with price 150)
    env.assertEqual(sql_result[1], rql_result[1])


# =============================================================================
# LIMIT/OFFSET Tests
# =============================================================================

def test_sql_limit(env):
    """FT.SQL with LIMIT"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    for i in range(10):
        conn.execute_command('HSET', f'doc{i}', 'name', f'Product {i}')

    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx LIMIT 3")

    # Result includes: [count, doc1, fields1, doc2, fields2, doc3, fields3]
    # Total count should still be 10, but only 3 docs returned
    env.assertGreaterEqual(sql_result[0], 10)
    doc_count = (len(sql_result) - 1) // 2
    env.assertEqual(doc_count, 3)


def test_sql_limit_offset(env):
    """FT.SQL with LIMIT n OFFSET m"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'SORTABLE')
    for i in range(10):
        conn.execute_command('HSET', f'doc{i}', 'name', f'Product {chr(65+i)}')

    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx ORDER BY name ASC LIMIT 3 OFFSET 2")
    rql_result = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'name', 'ASC', 'LIMIT', '2', '3')

    # Both should return same number of docs (3)
    sql_doc_count = (len(sql_result) - 1) // 2
    rql_doc_count = (len(rql_result) - 1) // 2
    env.assertEqual(sql_doc_count, rql_doc_count)
    env.assertEqual(sql_doc_count, 3)


# =============================================================================
# Error Handling Tests
# =============================================================================

def test_sql_syntax_error(env):
    """Invalid SQL should return error"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')

    try:
        env.cmd('FT.SQL', "SELEC * FROM idx")  # Typo
        env.assertTrue(False, "Should have raised error")
    except Exception as e:
        # Should contain error information
        error_str = str(e).lower()
        env.assertTrue('sql' in error_str or 'error' in error_str or 'syntax' in error_str)


def test_sql_nonexistent_index(env):
    """SQL query on non-existent index should error"""
    conn = _get_sql_connection(env)

    try:
        env.cmd('FT.SQL', "SELECT * FROM nonexistent_index_xyz")
        env.assertTrue(False, "Should have raised error")
    except Exception as e:
        # Expected to get an error about the index
        pass


def test_sql_empty_query(env):
    """Empty SQL should return error"""
    conn = _get_sql_connection(env)

    try:
        env.cmd('FT.SQL', "")
        env.assertTrue(False, "Should have raised error")
    except Exception as e:
        # Expected error
        pass


# =============================================================================
# Cache Behavior Tests
# =============================================================================

def test_sql_cache_hits(env):
    """Repeated queries should use cache (verify no errors)"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'name', 'Test')

    # Run same query multiple times
    for _ in range(5):
        result = env.cmd('FT.SQL', "SELECT * FROM idx")
        env.assertEqual(result[0], 1)  # Should always get same result


def test_sql_different_queries_cached_separately(env):
    """Different SQL queries should be cached separately"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'name', 'Test', 'price', '100')
    conn.execute_command('HSET', 'doc2', 'name', 'Other', 'price', '200')

    # Run different queries
    result1 = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price > 50")
    result2 = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price > 150")

    # Should get different result counts
    env.assertEqual(result1[0], 2)
    env.assertEqual(result2[0], 1)


# =============================================================================
# Aggregate Tests
# =============================================================================

def test_sql_group_by_having_parity(env):
    """FT.SQL GROUP BY/HAVING should match FT.AGGREGATE."""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'category', 'TAG', 'price', 'NUMERIC')

    docs = [
        ('doc1', 'electronics', '100'),
        ('doc2', 'electronics', '300'),
        ('doc3', 'furniture', '200'),
        ('doc4', 'furniture', '400'),
        ('doc5', 'clearance', '900'),
    ]
    for doc_id, category, price in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price "
        "FROM idx GROUP BY category HAVING COUNT(*) >= 2 ORDER BY category ASC"
    )
    native_result = env.cmd(
        'FT.AGGREGATE', 'idx', '*',
        'GROUPBY', '1', '@category',
        'REDUCE', 'COUNT', '0', 'AS', 'cnt',
        'REDUCE', 'AVG', '1', '@price', 'AS', 'avg_price',
        'FILTER', '@cnt>=2',
        'SORTBY', '2', '@category', 'ASC'
    )

    _assert_aggregate_parity(env, sql_result, native_result)


def test_sql_group_by_multi_order_by_parity(env):
    """Aggregate multi-column ORDER BY should route through FT.AGGREGATE correctly."""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'category', 'TAG')

    for doc_id, category in (
        ('doc1', 'alpha'),
        ('doc2', 'alpha'),
        ('doc3', 'beta'),
        ('doc4', 'beta'),
        ('doc5', 'beta'),
        ('doc6', 'gamma'),
        ('doc7', 'gamma'),
        ('doc8', 'gamma'),
    ):
        conn.execute_command('HSET', doc_id, 'category', category)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, COUNT(*) AS cnt "
        "FROM idx GROUP BY category ORDER BY cnt DESC, category ASC"
    )
    native_result = env.cmd(
        'FT.AGGREGATE', 'idx', '*',
        'GROUPBY', '1', '@category',
        'REDUCE', 'COUNT', '0', 'AS', 'cnt',
        'SORTBY', '4', '@cnt', 'DESC', '@category', 'ASC'
    )

    _assert_aggregate_parity(env, sql_result, native_result)


# =============================================================================
# Complex Query Tests
# =============================================================================

def test_sql_complex_query(env):
    """Complex SQL query with multiple clauses"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'products',
        'SCHEMA', 'name', 'TEXT', 'SORTABLE',
        'price', 'NUMERIC', 'SORTABLE',
        'category', 'TAG')

    conn.execute_command('HSET', 'p1', 'name', 'Laptop', 'price', '1000', 'category', 'electronics')
    conn.execute_command('HSET', 'p2', 'name', 'Phone', 'price', '500', 'category', 'electronics')
    conn.execute_command('HSET', 'p3', 'name', 'Desk', 'price', '200', 'category', 'furniture')
    conn.execute_command('HSET', 'p4', 'name', 'Chair', 'price', '150', 'category', 'furniture')

    sql_result = env.cmd('FT.SQL',
        "SELECT name, price FROM products WHERE price > 100 ORDER BY price DESC LIMIT 3")

    # Should get at least 3 matches (all 4 products have price > 100)
    env.assertGreaterEqual(sql_result[0], 3)
    # Should return 3 docs due to LIMIT
    doc_count = (len(sql_result) - 1) // 2
    env.assertEqual(doc_count, 3)


def test_sql_select_fields_with_where(env):
    """FT.SQL with both SELECT fields and WHERE clause"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC', 'stock', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'name', 'Item A', 'price', '100', 'stock', '10')
    conn.execute_command('HSET', 'doc2', 'name', 'Item B', 'price', '200', 'stock', '20')

    sql_result = env.cmd('FT.SQL', "SELECT name, stock FROM idx WHERE price > 50")
    rql_result = env.cmd('FT.SEARCH', 'idx', '@price:[(50 +inf]', 'RETURN', '2', 'name', 'stock')

    env.assertEqual(sql_result[0], rql_result[0])


def test_sql_hybrid_query(env):
    """FT.SQL Hybrid queries should match the live FT.HYBRID grammar."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'category', 'TAG',
        'embedding', 'VECTOR', 'FLAT', 6, 'TYPE', 'FLOAT32', 'DIM', 2, 'DISTANCE_METRIC', 'L2'
    )
    conn.execute_command('HSET', 'doc1', 'category', 'electronics',
                         'embedding', np.array([0.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc2', 'category', 'electronics',
                         'embedding', np.array([1.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc3', 'category', 'accessories',
                         'embedding', np.array([0.0, 1.0]).astype(np.float32).tobytes())
    waitForIndex(env, 'idx')

    sql_response = env.cmd(
        'FT.SQL',
        "SELECT * FROM idx WHERE category = 'electronics' "
        "ORDER BY embedding <-> '[0.0, 0.0]' LIMIT 2 "
        "OPTION (vector_weight = 0.7, text_weight = 0.3)"
    )

    hybrid_response = env.cmd(
        'FT.HYBRID', 'idx',
        'SEARCH', '@category:{electronics}',
        'VSIM', '@embedding', '$BLOB',
        'KNN', '2', 'K', '2',
        'COMBINE', 'LINEAR', '4', 'ALPHA', '0.7', 'BETA', '0.3',
        'LIMIT', '0', '2',
        'PARAMS', '2', 'BLOB', np.array([0.0, 0.0]).astype(np.float32).tobytes()
    )

    _assert_hybrid_parity(env, sql_response, hybrid_response)


def test_sql_vector_knn_query(env):
    """FT.SQL vector KNN should match FT.SEARCH KNN execution."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'category', 'TAG',
        'embedding', 'VECTOR', 'FLAT', 6, 'TYPE', 'FLOAT32', 'DIM', 2, 'DISTANCE_METRIC', 'L2'
    )

    docs = [
        ('doc1', 'electronics', np.array([0.0, 0.0]).astype(np.float32).tobytes()),
        ('doc2', 'electronics', np.array([1.0, 0.0]).astype(np.float32).tobytes()),
        ('doc3', 'electronics', np.array([0.0, 2.0]).astype(np.float32).tobytes()),
        ('doc4', 'accessories', np.array([0.0, 0.0]).astype(np.float32).tobytes()),
    ]
    for doc_id, category, vector in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'embedding', vector)
    waitForIndex(env, 'idx')

    query_vector = np.array([0.1, 0.0]).astype(np.float32)
    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category FROM idx WHERE category = 'electronics' "
        "ORDER BY embedding <-> '[0.1, 0.0]' LIMIT 2"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx',
        '@category:{electronics}=>[KNN 2 @embedding $BLOB]',
        'PARAMS', '2', 'BLOB', query_vector.tobytes(),
        'RETURN', '1', 'category',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


def test_sql_hybrid_query_default_text_weight(env):
    """FT.SQL Hybrid should default text_weight to 0.5 when only vector_weight is set."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'SCHEMA',
        'category', 'TAG',
        'embedding', 'VECTOR', 'FLAT', 6, 'TYPE', 'FLOAT32', 'DIM', 2, 'DISTANCE_METRIC', 'L2'
    )
    conn.execute_command('HSET', 'doc1', 'category', 'electronics',
                         'embedding', np.array([0.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc2', 'category', 'electronics',
                         'embedding', np.array([1.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc3', 'category', 'accessories',
                         'embedding', np.array([0.0, 1.0]).astype(np.float32).tobytes())
    waitForIndex(env, 'idx')

    query_vector = np.array([0.0, 0.0]).astype(np.float32)
    sql_response = env.cmd(
        'FT.SQL',
        "SELECT * FROM idx WHERE category = 'electronics' "
        "ORDER BY embedding <-> '[0.0, 0.0]' LIMIT 2 "
        "OPTION (vector_weight = 0.6)"
    )
    hybrid_response = env.cmd(
        'FT.HYBRID', 'idx',
        'SEARCH', '@category:{electronics}',
        'VSIM', '@embedding', '$BLOB',
        'KNN', '2', 'K', '2',
        'COMBINE', 'LINEAR', '4', 'ALPHA', '0.6', 'BETA', '0.5',
        'LIMIT', '0', '2',
        'PARAMS', '2', 'BLOB', query_vector.tobytes()
    )

    _assert_hybrid_parity(env, sql_response, hybrid_response)


def test_sql_case_insensitive_keywords(env):
    """SQL keywords should be case-insensitive"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'name', 'Test')

    # Mix of uppercase and lowercase keywords
    result = env.cmd('FT.SQL', "select * from idx limit 10")
    env.assertEqual(result[0], 1)


def test_sql_multiple_documents(env):
    """FT.SQL should handle many documents correctly"""
    conn = _get_sql_connection(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'value', 'NUMERIC')

    # Add 50 documents
    for i in range(50):
        conn.execute_command('HSET', f'doc{i}', 'name', f'Item {i}', 'value', str(i * 10))

    # Query all
    result_all = env.cmd('FT.SQL', "SELECT * FROM idx LIMIT 100")
    env.assertEqual(result_all[0], 50)

    # Query with filter
    result_filtered = env.cmd('FT.SQL', "SELECT * FROM idx WHERE value >= 250 LIMIT 100")
    # value >= 250 means i >= 25, so 25 docs (i=25 to i=49)
    env.assertEqual(result_filtered[0], 25)


def test_sql_tag_value_special_chars_are_escaped(env):
    conn = _get_sql_connection(env)

    special_value = r"a{b}|c\\d}"
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'status', 'TAG')
    conn.execute_command('HSET', 'doc1', 'status', special_value)
    conn.execute_command('HSET', 'doc2', 'status', 'plain')

    sql_result = env.cmd('FT.SQL', f"SELECT * FROM idx WHERE status = '{special_value}'")

    env.assertEqual(sql_result[0], 1)


def test_sql_text_equality_is_rejected(env):
    _enable_sql(env)
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'title', 'TEXT')

    env.expect('FT.SQL', "SELECT * FROM idx WHERE title = 'redis'").error() \
        .contains('TEXT field').contains('MATCH')


def test_sql_option_without_vector_is_rejected(env):
    _enable_sql(env)
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'category', 'TAG')

    env.expect(
        'FT.SQL',
        "SELECT * FROM idx WHERE category = 'electronics' OPTION (vector_weight = 0.7)"
    ).error().contains('requires a vector search')


def test_sql_multi_order_by_plain_search_is_rejected(env):
    _enable_sql(env)
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'SORTABLE', 'price', 'NUMERIC', 'SORTABLE')

    env.expect(
        'FT.SQL',
        "SELECT * FROM idx ORDER BY price DESC, name ASC"
    ).error().contains('Multiple ORDER BY columns are not supported by FT.SEARCH')


@skip(no_json=True)
def test_sql_json_index_search_parity(env):
    """FT.SQL should work against JSON indexes with aliased fields."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'JSON', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        '$.name', 'AS', 'name', 'TEXT',
        '$.price', 'AS', 'price', 'NUMERIC', 'SORTABLE',
        '$.category', 'AS', 'category', 'TAG'
    )

    docs = {
        'doc:1': {'name': 'Laptop', 'price': 1000, 'category': 'electronics'},
        'doc:2': {'name': 'Phone', 'price': 500, 'category': 'electronics'},
        'doc:3': {'name': 'Desk', 'price': 200, 'category': 'furniture'},
    }
    for doc_id, payload in docs.items():
        conn.execute_command('JSON.SET', doc_id, '$', json.dumps(payload))
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT name, price FROM idx WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '@category:{electronics}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'name', 'price',
        'LIMIT', '0', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


@skip(cluster=False)
def test_sql_cluster_search_parity(env):
    """FT.SQL should be available on the coordinator/public command path."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'HASH', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        'name', 'TEXT', 'SORTABLE',
        'category', 'TAG',
        'price', 'NUMERIC', 'SORTABLE'
    )

    docs = [
        ('doc:1', 'Laptop', 'electronics', '1000'),
        ('doc:2', 'Phone', 'electronics', '500'),
        ('doc:3', 'Tablet', 'electronics', '750'),
        ('doc:4', 'Desk', 'furniture', '200'),
    ]
    for doc_id, name, category, price in docs:
        conn.execute_command('HSET', doc_id, 'name', name, 'category', category, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT name, price FROM idx WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '@category:{electronics}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'name', 'price',
        'LIMIT', '0', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


@skip(cluster=False)
def test_sql_cluster_aggregate_parity(env):
    """FT.SQL aggregates should route through the coordinator/public command path."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'HASH', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        'category', 'TAG',
        'price', 'NUMERIC'
    )

    docs = [
        ('doc:1', 'electronics', '100'),
        ('doc:2', 'electronics', '300'),
        ('doc:3', 'furniture', '200'),
        ('doc:4', 'furniture', '400'),
        ('doc:5', 'clearance', '900'),
    ]
    for doc_id, category, price in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'price', price)
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price "
        "FROM idx GROUP BY category HAVING COUNT(*) >= 2 ORDER BY category ASC"
    )
    native_result = env.cmd(
        'FT.AGGREGATE', 'idx', '*',
        'GROUPBY', '1', '@category',
        'REDUCE', 'COUNT', '0', 'AS', 'cnt',
        'REDUCE', 'AVG', '1', '@price', 'AS', 'avg_price',
        'FILTER', '@cnt>=2',
        'SORTBY', '2', '@category', 'ASC'
    )

    _assert_aggregate_parity(env, sql_result, native_result)


@skip(cluster=False)
def test_sql_cluster_vector_knn_parity(env):
    """FT.SQL vector KNN should work on the coordinator/public command path."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'HASH', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        'category', 'TAG',
        'embedding', 'VECTOR', 'FLAT', 6, 'TYPE', 'FLOAT32', 'DIM', 2, 'DISTANCE_METRIC', 'L2'
    )

    docs = [
        ('doc:1', 'electronics', np.array([0.0, 0.0]).astype(np.float32).tobytes()),
        ('doc:2', 'electronics', np.array([1.0, 0.0]).astype(np.float32).tobytes()),
        ('doc:3', 'electronics', np.array([0.0, 2.0]).astype(np.float32).tobytes()),
        ('doc:4', 'accessories', np.array([0.0, 0.0]).astype(np.float32).tobytes()),
    ]
    for doc_id, category, vector in docs:
        conn.execute_command('HSET', doc_id, 'category', category, 'embedding', vector)
    waitForIndex(env, 'idx')

    query_vector = np.array([0.1, 0.0]).astype(np.float32)
    sql_result = env.cmd(
        'FT.SQL',
        "SELECT category FROM idx WHERE category = 'electronics' "
        "ORDER BY embedding <-> '[0.1, 0.0]' LIMIT 2"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx',
        '@category:{electronics}=>[KNN 2 @embedding $BLOB]',
        'PARAMS', '2', 'BLOB', query_vector.tobytes(),
        'RETURN', '1', 'category',
        'DIALECT', '2'
    )

    _assert_search_parity(env, sql_result, native_result)


@skip(cluster=False)
def test_sql_cluster_hybrid_parity(env):
    """FT.SQL weighted Hybrid should work on the coordinator/public command path."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'HASH', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        'category', 'TAG',
        'embedding', 'VECTOR', 'FLAT', 6, 'TYPE', 'FLOAT32', 'DIM', 2, 'DISTANCE_METRIC', 'L2'
    )

    conn.execute_command('HSET', 'doc:1', 'category', 'electronics',
                         'embedding', np.array([0.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc:2', 'category', 'electronics',
                         'embedding', np.array([1.0, 0.0]).astype(np.float32).tobytes())
    conn.execute_command('HSET', 'doc:3', 'category', 'accessories',
                         'embedding', np.array([0.0, 1.0]).astype(np.float32).tobytes())
    waitForIndex(env, 'idx')

    query_vector = np.array([0.0, 0.0]).astype(np.float32)
    sql_result = env.cmd(
        'FT.SQL',
        "SELECT * FROM idx WHERE category = 'electronics' "
        "ORDER BY embedding <-> '[0.0, 0.0]' LIMIT 2 "
        "OPTION (vector_weight = 0.7, text_weight = 0.3)"
    )
    native_result = env.cmd(
        'FT.HYBRID', 'idx',
        'SEARCH', '@category:{electronics}',
        'VSIM', '@embedding', '$BLOB',
        'KNN', '2', 'K', '2',
        'COMBINE', 'LINEAR', '4', 'ALPHA', '0.7', 'BETA', '0.3',
        'LIMIT', '0', '2',
        'PARAMS', '2', 'BLOB', query_vector.tobytes()
    )

    _assert_hybrid_parity(env, sql_result, native_result)


@skip(no_json=True)
@skip(cluster=False)
def test_sql_cluster_json_index_search_parity(env):
    """FT.SQL should work against JSON indexes on the coordinator/public command path."""
    conn = _get_sql_connection(env)

    env.cmd(
        'FT.CREATE', 'idx', 'ON', 'JSON', 'PREFIX', '1', 'doc:',
        'SCHEMA',
        '$.name', 'AS', 'name', 'TEXT',
        '$.price', 'AS', 'price', 'NUMERIC', 'SORTABLE',
        '$.category', 'AS', 'category', 'TAG'
    )

    docs = {
        'doc:1': {'name': 'Laptop', 'price': 1000, 'category': 'electronics'},
        'doc:2': {'name': 'Phone', 'price': 500, 'category': 'electronics'},
        'doc:3': {'name': 'Desk', 'price': 200, 'category': 'furniture'},
    }
    for doc_id, payload in docs.items():
        conn.execute_command('JSON.SET', doc_id, '$', json.dumps(payload))
    waitForIndex(env, 'idx')

    sql_result = env.cmd(
        'FT.SQL',
        "SELECT name, price FROM idx WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
    )
    native_result = env.cmd(
        'FT.SEARCH', 'idx', '@category:{electronics}',
        'SORTBY', 'price', 'ASC',
        'RETURN', '2', 'name', 'price',
        'LIMIT', '0', '2'
    )

    _assert_search_parity(env, sql_result, native_result)
