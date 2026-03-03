"""
Test DATETIME field type functionality.
"""
from common import *
from RLTest import Env


def test_datetime_field_creation(env):
    """Test creating an index with a DATETIME field."""
    conn = getConnectionByEnv(env)
    
    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'created', 'DATETIME').ok()
    
    # Verify index was created
    info = env.cmd('FT.INFO', 'idx')
    env.assertIn('created', str(info))


def test_datetime_field_sortable(env):
    """Test creating a sortable DATETIME field."""
    conn = getConnectionByEnv(env)
    
    # Create index with sortable DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME', 'SORTABLE').ok()
    
    # Verify index was created
    info = env.cmd('FT.INFO', 'idx')
    env.assertIn('timestamp', str(info))


def test_datetime_field_indexmissing(env):
    """Test creating a DATETIME field with INDEXMISSING option."""
    conn = getConnectionByEnv(env)
    
    # Create index with INDEXMISSING DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'updated', 'DATETIME', 'INDEXMISSING').ok()
    
    # Verify index was created
    info = env.cmd('FT.INFO', 'idx')
    env.assertIn('updated', str(info))


def test_datetime_field_basic_indexing(env):
    """Test basic indexing with DATETIME field (using numeric timestamp)."""
    conn = getConnectionByEnv(env)
    
    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'created', 'DATETIME').ok()
    
    # Add document with Unix timestamp (numeric value)
    # For now, DATETIME fields accept numeric timestamps like NUMERIC fields
    env.expect('HSET', 'doc1', 'created', '1704067200').equal(1)  # 2024-01-01 00:00:00 UTC
    env.expect('HSET', 'doc2', 'created', '1735689600').equal(1)  # 2025-01-01 00:00:00 UTC
    
    # Search for documents (using numeric range syntax for now)
    # DATETIME uses numeric infrastructure, so numeric queries work
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1704067200 1735689600]')
    env.assertEqual(res[0], 2)  # Should find both documents


def test_datetime_multiple_fields(env):
    """Test index with multiple DATETIME fields."""
    conn = getConnectionByEnv(env)
    
    # Create index with multiple DATETIME fields
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 
               'created', 'DATETIME',
               'updated', 'DATETIME',
               'deleted', 'DATETIME').ok()
    
    # Verify index was created
    info = env.cmd('FT.INFO', 'idx')
    env.assertIn('created', str(info))
    env.assertIn('updated', str(info))
    env.assertIn('deleted', str(info))


def test_datetime_with_other_fields(env):
    """Test DATETIME field alongside other field types."""
    conn = getConnectionByEnv(env)
    
    # Create index with mixed field types
    env.expect('FT.CREATE', 'idx', 'SCHEMA',
               'title', 'TEXT',
               'price', 'NUMERIC',
               'created', 'DATETIME',
               'tags', 'TAG').ok()
    
    # Add a document
    env.expect('HSET', 'doc1',
               'title', 'Test Document',
               'price', '99.99',
               'created', '1704067200',
               'tags', 'test,datetime').equal(4)
    
    # Search by DATETIME field
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1704067200 1704067200]')
    env.assertEqual(res[0], 1)
    
    # Search by TEXT field
    res = env.cmd('FT.SEARCH', 'idx', '@title:Test')
    env.assertEqual(res[0], 1)

