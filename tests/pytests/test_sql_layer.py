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

from common import *


# =============================================================================
# Basic SELECT Tests
# =============================================================================

def test_sql_select_star(env):
    """FT.SQL 'SELECT * FROM idx' should match FT.SEARCH idx '*'"""
    conn = getConnectionByEnv(env)
    
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
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC', 'category', 'TAG')
    conn.execute_command('HSET', 'doc1', 'name', 'Test', 'price', '100', 'category', 'electronics')
    
    sql_result = env.cmd('FT.SQL', "SELECT name, price FROM idx")
    rql_result = env.cmd('FT.SEARCH', 'idx', '*', 'RETURN', '2', 'name', 'price')
    
    env.assertEqual(sql_result[0], rql_result[0])


# =============================================================================
# WHERE Clause Tests
# =============================================================================

def test_sql_where_equality(env):
    """FT.SQL with WHERE field = 'value' on TEXT field"""
    conn = getConnectionByEnv(env)

    # Use TEXT field - SQL equality translates to RQL exact match on TEXT fields
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'status', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'status', 'active')
    conn.execute_command('HSET', 'doc2', 'status', 'inactive')

    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE status = 'active'")

    # Should find exactly the doc with 'active'
    env.assertEqual(sql_result[0], 1)


def test_sql_where_greater_than(env):
    """FT.SQL with WHERE field > value"""
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price > 75")
    
    env.assertEqual(sql_result[0], 2)  # doc2 and doc3


def test_sql_where_less_than(env):
    """FT.SQL with WHERE field < value"""
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price < 75")
    
    env.assertEqual(sql_result[0], 1)  # Only doc1


def test_sql_where_greater_equal(env):
    """FT.SQL with WHERE field >= value"""
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price >= 100")
    
    env.assertEqual(sql_result[0], 2)  # doc2 and doc3


def test_sql_where_less_equal(env):
    """FT.SQL with WHERE field <= value"""
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price <= 100")
    
    env.assertEqual(sql_result[0], 2)  # doc1 and doc2


def test_sql_where_between(env):
    """FT.SQL with WHERE field BETWEEN a AND b"""
    conn = getConnectionByEnv(env)
    
    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'price', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'price', '50')
    conn.execute_command('HSET', 'doc2', 'price', '100')
    conn.execute_command('HSET', 'doc3', 'price', '150')
    
    sql_result = env.cmd('FT.SQL', "SELECT * FROM idx WHERE price BETWEEN 75 AND 125")

    env.assertEqual(sql_result[0], 1)  # Only doc2 (price=100)


# =============================================================================
# ORDER BY Tests
# =============================================================================

def test_sql_order_by_asc(env):
    """FT.SQL with ORDER BY field ASC"""
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

    try:
        env.cmd('FT.SQL', "SELECT * FROM nonexistent_index_xyz")
        env.assertTrue(False, "Should have raised error")
    except Exception as e:
        # Expected to get an error about the index
        pass


def test_sql_empty_query(env):
    """Empty SQL should return error"""
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'name', 'Test')

    # Run same query multiple times
    for _ in range(5):
        result = env.cmd('FT.SQL', "SELECT * FROM idx")
        env.assertEqual(result[0], 1)  # Should always get same result


def test_sql_different_queries_cached_separately(env):
    """Different SQL queries should be cached separately"""
    conn = getConnectionByEnv(env)

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
# Complex Query Tests
# =============================================================================

def test_sql_complex_query(env):
    """Complex SQL query with multiple clauses"""
    conn = getConnectionByEnv(env)

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
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT', 'price', 'NUMERIC', 'stock', 'NUMERIC')
    conn.execute_command('HSET', 'doc1', 'name', 'Item A', 'price', '100', 'stock', '10')
    conn.execute_command('HSET', 'doc2', 'name', 'Item B', 'price', '200', 'stock', '20')

    sql_result = env.cmd('FT.SQL', "SELECT name, stock FROM idx WHERE price > 50")
    rql_result = env.cmd('FT.SEARCH', 'idx', '@price:[(50 +inf]', 'RETURN', '2', 'name', 'stock')

    env.assertEqual(sql_result[0], rql_result[0])


def test_sql_case_insensitive_keywords(env):
    """SQL keywords should be case-insensitive"""
    conn = getConnectionByEnv(env)

    env.cmd('FT.CREATE', 'idx', 'SCHEMA', 'name', 'TEXT')
    conn.execute_command('HSET', 'doc1', 'name', 'Test')

    # Mix of uppercase and lowercase keywords
    result = env.cmd('FT.SQL', "select * from idx limit 10")
    env.assertEqual(result[0], 1)


def test_sql_multiple_documents(env):
    """FT.SQL should handle many documents correctly"""
    conn = getConnectionByEnv(env)

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

