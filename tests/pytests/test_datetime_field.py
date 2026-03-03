"""
Test DATETIME field type functionality.

DATETIME fields use the numeric infrastructure and accept:
- Unix timestamps (numeric values)
- ISO-8601 formatted date/datetime strings
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


def test_datetime_range_queries(env):
    """Test range queries on DATETIME fields."""
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME').ok()

    # Add documents with various timestamps
    env.expect('HSET', 'doc1', 'timestamp', '1609459200').equal(1)  # 2021-01-01 00:00:00 UTC
    env.expect('HSET', 'doc2', 'timestamp', '1640995200').equal(1)  # 2022-01-01 00:00:00 UTC
    env.expect('HSET', 'doc3', 'timestamp', '1672531200').equal(1)  # 2023-01-01 00:00:00 UTC
    env.expect('HSET', 'doc4', 'timestamp', '1704067200').equal(1)  # 2024-01-01 00:00:00 UTC
    env.expect('HSET', 'doc5', 'timestamp', '1735689600').equal(1)  # 2025-01-01 00:00:00 UTC

    # Test exact match
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1704067200 1704067200]')
    env.assertEqual(res[0], 1)
    env.assertIn('doc4', res)

    # Test range query (2022-2024)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1640995200 1704067200]')
    env.assertEqual(res[0], 3)

    # Test open-ended range (from 2023 onwards)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1672531200 +inf]')
    env.assertEqual(res[0], 3)

    # Test open-ended range (up to 2022)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-inf 1640995200]')
    env.assertEqual(res[0], 2)


def test_datetime_sorting(env):
    """Test sorting by DATETIME field."""
    conn = getConnectionByEnv(env)

    # Create index with sortable DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME', 'SORTABLE').ok()

    # Add documents in non-chronological order
    env.expect('HSET', 'doc1', 'timestamp', '1704067200').equal(1)  # 2024-01-01
    env.expect('HSET', 'doc2', 'timestamp', '1609459200').equal(1)  # 2021-01-01
    env.expect('HSET', 'doc3', 'timestamp', '1672531200').equal(1)  # 2023-01-01
    env.expect('HSET', 'doc4', 'timestamp', '1640995200').equal(1)  # 2022-01-01

    # Sort ascending (oldest first)
    res = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'timestamp', 'ASC')
    env.assertEqual(res[0], 4)
    env.assertEqual(res[1], 'doc2')  # 2021
    env.assertEqual(res[3], 'doc4')  # 2022
    env.assertEqual(res[5], 'doc3')  # 2023
    env.assertEqual(res[7], 'doc1')  # 2024

    # Sort descending (newest first)
    res = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'timestamp', 'DESC')
    env.assertEqual(res[0], 4)
    env.assertEqual(res[1], 'doc1')  # 2024
    env.assertEqual(res[3], 'doc3')  # 2023
    env.assertEqual(res[5], 'doc4')  # 2022
    env.assertEqual(res[7], 'doc2')  # 2021


def test_datetime_large_timestamps(env):
    """Test DATETIME field with large timestamps (year 2100+)."""
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME').ok()

    # Add documents with far-future timestamps
    env.expect('HSET', 'doc1', 'timestamp', '4102444800').equal(1)  # 2100-01-01 00:00:00 UTC
    env.expect('HSET', 'doc2', 'timestamp', '4133980800').equal(1)  # 2101-01-01 00:00:00 UTC
    env.expect('HSET', 'doc3', 'timestamp', '7258118400').equal(1)  # 2200-01-01 00:00:00 UTC

    # Test exact match
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[4102444800 4102444800]')
    env.assertEqual(res[0], 1)
    env.assertIn('doc1', res)

    # Test range query
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[4102444800 4133980800]')
    env.assertEqual(res[0], 2)

    # Test all future dates
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[4102444800 +inf]')
    env.assertEqual(res[0], 3)


def test_datetime_negative_timestamps(env):
    """Test DATETIME field with negative timestamps (before Unix epoch 1970)."""
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME').ok()

    # Add documents with pre-1970 timestamps
    env.expect('HSET', 'doc1', 'timestamp', '-315619200').equal(1)  # 1960-01-01 00:00:00 UTC
    env.expect('HSET', 'doc2', 'timestamp', '-631152000').equal(1)  # 1950-01-01 00:00:00 UTC
    env.expect('HSET', 'doc3', 'timestamp', '0').equal(1)            # 1970-01-01 00:00:00 UTC (epoch)
    env.expect('HSET', 'doc4', 'timestamp', '946684800').equal(1)    # 2000-01-01 00:00:00 UTC

    # Test exact match on negative timestamp
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-315619200 -315619200]')
    env.assertEqual(res[0], 1)
    env.assertIn('doc1', res)

    # Test range query with negative timestamps
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-631152000 -315619200]')
    env.assertEqual(res[0], 2)

    # Test range spanning epoch
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-315619200 946684800]')
    env.assertEqual(res[0], 3)

    # Test all timestamps
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-inf +inf]')
    env.assertEqual(res[0], 4)


def test_datetime_edge_cases(env):
    """Test DATETIME field edge cases."""
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME').ok()

    # Test zero timestamp (Unix epoch)
    env.expect('HSET', 'doc1', 'timestamp', '0').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[0 0]')
    env.assertEqual(res[0], 1)

    # Test very small positive timestamp
    env.expect('HSET', 'doc2', 'timestamp', '1').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1 1]')
    env.assertEqual(res[0], 1)

    # Test very small negative timestamp
    env.expect('HSET', 'doc3', 'timestamp', '-1').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[-1 -1]')
    env.assertEqual(res[0], 1)


def test_datetime_combined_queries(env):
    """Test combining DATETIME queries with other field types."""
    conn = getConnectionByEnv(env)

    # Create index with mixed field types
    env.expect('FT.CREATE', 'idx', 'SCHEMA',
               'title', 'TEXT',
               'status', 'TAG',
               'created', 'DATETIME',
               'priority', 'NUMERIC').ok()

    # Add documents
    env.expect('HSET', 'doc1',
               'title', 'Bug Report',
               'status', 'open',
               'created', '1704067200',  # 2024-01-01
               'priority', '5').equal(4)

    env.expect('HSET', 'doc2',
               'title', 'Feature Request',
               'status', 'closed',
               'created', '1672531200',  # 2023-01-01
               'priority', '3').equal(4)

    env.expect('HSET', 'doc3',
               'title', 'Bug Fix',
               'status', 'open',
               'created', '1704067200',  # 2024-01-01
               'priority', '8').equal(4)

    # Query: open items created in 2024
    res = env.cmd('FT.SEARCH', 'idx', '@status:{open} @created:[1704067200 +inf]')
    env.assertEqual(res[0], 2)

    # Query: high priority (>5) items created in 2024
    res = env.cmd('FT.SEARCH', 'idx', '@priority:[5 +inf] @created:[1704067200 +inf]')
    env.assertEqual(res[0], 2)

    # Query: items with "Bug" in title created after 2023
    res = env.cmd('FT.SEARCH', 'idx', '@title:Bug @created:[1672531200 +inf]')
    env.assertEqual(res[0], 2)


def test_datetime_sortable_with_missing_values(env):
    """Test DATETIME SORTABLE field with missing values."""
    conn = getConnectionByEnv(env)

    # Create index with sortable DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME', 'SORTABLE').ok()

    # Add documents, some with missing timestamp
    env.expect('HSET', 'doc1', 'timestamp', '1704067200').equal(1)
    env.expect('HSET', 'doc2', 'other', 'value').equal(1)  # No timestamp
    env.expect('HSET', 'doc3', 'timestamp', '1672531200').equal(1)

    # Sort by timestamp - documents with missing values should appear at the end
    res = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'timestamp', 'ASC')
    env.assertEqual(res[0], 3)


def test_datetime_indexmissing_functionality(env):
    """Test DATETIME field with INDEXMISSING option."""
    conn = getConnectionByEnv(env)

    # Create index with INDEXMISSING DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME', 'INDEXMISSING').ok()

    # Add documents with and without timestamp
    env.expect('HSET', 'doc1', 'timestamp', '1704067200').equal(1)
    env.expect('HSET', 'doc2', 'other', 'value').equal(1)  # No timestamp
    env.expect('HSET', 'doc3', 'timestamp', '1672531200').equal(1)

    # Search for all documents (should include those with missing timestamp)
    res = env.cmd('FT.SEARCH', 'idx', '*')
    env.assertEqual(res[0], 3)


def test_datetime_limitations(env):
    """Document current limitations of DATETIME field type.

    This test documents features that are NOT YET supported:
    - ISO-8601 date string parsing (e.g., "2024-01-01T00:00:00Z")
    - Relative date expressions (e.g., "now", "today", "yesterday")
    - Date arithmetic in queries

    Currently, DATETIME fields only accept Unix timestamps (numeric values).
    """
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME').ok()

    # ISO-8601 strings are NOT YET supported - they will be stored as strings
    # and won't be indexed as timestamps
    env.expect('HSET', 'doc1', 'timestamp', '2024-01-01T00:00:00Z').equal(1)

    # This query won't find the document because the value wasn't parsed as a timestamp
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1704067200 1704067200]')
    env.assertEqual(res[0], 0)  # Not found

    # Only numeric Unix timestamps work
    env.expect('HSET', 'doc2', 'timestamp', '1704067200').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@timestamp:[1704067200 1704067200]')
    env.assertEqual(res[0], 1)  # Found


def test_datetime_iso8601_parsing(env):
    """Test ISO-8601 date/datetime parsing for DATETIME fields."""
    conn = getConnectionByEnv(env)

    # Create index with DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'created', 'DATETIME').ok()

    # Test date-only format (YYYY-MM-DD) - should be midnight UTC
    env.expect('HSET', 'doc1', 'created', '2024-01-01').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1704067200 1704067200]')
    env.assertEqual(res[0], 1)  # 2024-01-01 00:00:00 UTC = 1704067200

    # Test datetime with UTC timezone
    env.expect('HSET', 'doc2', 'created', '2024-03-15T10:30:00Z').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1710498600 1710498600]')
    env.assertEqual(res[0], 1)  # 2024-03-15 10:30:00 UTC = 1710498600

    # Test datetime with timezone offset
    env.expect('HSET', 'doc3', 'created', '2024-03-15T15:30:00+05:00').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1710498600 1710498600]')
    env.assertEqual(res[0], 2)  # Same as doc2 (15:30+05:00 = 10:30 UTC)

    # Test that numeric timestamps still work
    env.expect('HSET', 'doc4', 'created', '1735689600').equal(1)
    res = env.cmd('FT.SEARCH', 'idx', '@created:[1735689600 1735689600]')
    env.assertEqual(res[0], 1)  # 2025-01-01 00:00:00 UTC


def test_datetime_iso8601_sortable(env):
    """Test ISO-8601 parsing with sortable DATETIME fields."""
    conn = getConnectionByEnv(env)

    # Create index with sortable DATETIME field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'timestamp', 'DATETIME', 'SORTABLE').ok()

    # Add documents with ISO-8601 dates
    env.expect('HSET', 'doc1', 'timestamp', '2024-01-01').equal(1)
    env.expect('HSET', 'doc2', 'timestamp', '2024-06-15T12:00:00Z').equal(1)
    env.expect('HSET', 'doc3', 'timestamp', '2024-12-31T23:59:59Z').equal(1)

    # Search and sort by timestamp
    res = env.cmd('FT.SEARCH', 'idx', '*', 'SORTBY', 'timestamp', 'ASC')
    env.assertEqual(res[0], 3)
    # Verify order: doc1 < doc2 < doc3
    env.assertIn('doc1', str(res[1]))
    env.assertIn('doc2', str(res[3]))
    env.assertIn('doc3', str(res[5]))

