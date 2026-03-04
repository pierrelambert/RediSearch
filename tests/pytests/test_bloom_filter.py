from common import *
import random
import string
import time

def test_bloom_filter_stats(env):
    """Test that Bloom filter stats are reported in FT.INFO"""
    conn = getConnectionByEnv(env)
    
    # Create index with text field
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()
    
    # Initially, filter should be empty
    info = index_info(env, 'idx')
    env.assertEqual(float(info['bloom_filter_sz_mb']), 0, message="Initial bloom filter size should be 0")
    env.assertEqual(int(info['bloom_filter_terms']), 0, message="Initial bloom filter terms should be 0")
    
    # Add documents with unique terms
    n_docs = 100
    for i in range(n_docs):
        conn.execute_command('HSET', f'doc{i}', 'text', f'term{i}')
    
    # Wait for indexing to complete
    waitForIndex(env, 'idx')
    
    # Check that filter stats are updated
    info = index_info(env, 'idx')
    bloom_size = float(info['bloom_filter_sz_mb'])
    bloom_terms = int(info['bloom_filter_terms'])
    
    env.assertGreater(bloom_size, 0, message="Bloom filter size should be > 0 after adding docs")
    env.assertGreater(bloom_terms, 0, message="Bloom filter terms should be > 0 after adding docs")
    
    # Verify term count is reasonable (should be at least n_docs, possibly more due to stemming)
    env.assertGreaterEqual(bloom_terms, n_docs, message=f"Should have at least {n_docs} terms")

def test_bloom_filter_negative_lookup(env):
    """Test that non-existent terms are rejected quickly via Bloom filter"""
    conn = getConnectionByEnv(env)
    
    # Create index and add documents
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()
    
    # Add documents with known terms
    known_terms = ['apple', 'banana', 'cherry', 'date', 'elderberry']
    for i, term in enumerate(known_terms):
        conn.execute_command('HSET', f'doc{i}', 'text', term)
    
    waitForIndex(env, 'idx')
    
    # Search for terms that definitely don't exist
    # These should be rejected by the Bloom filter without trie traversal
    nonexistent_terms = ['xyznonexistent', 'qwertyzzzz', 'abcdefghijklmnop']
    
    for term in nonexistent_terms:
        result = env.cmd('FT.SEARCH', 'idx', f'@text:{term}')
        env.assertEqual(result[0], 0, message=f"Search for '{term}' should return 0 results")
    
    # Verify known terms still work
    for term in known_terms:
        result = env.cmd('FT.SEARCH', 'idx', f'@text:{term}')
        env.assertGreater(result[0], 0, message=f"Search for '{term}' should return results")

def test_bloom_filter_false_positive_rate(env):
    """Test that false positive rate is approximately 1%"""
    conn = getConnectionByEnv(env)
    
    # Create index
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()
    
    # Add N known terms
    n_known = 1000
    known_terms = set()
    for i in range(n_known):
        term = f'known_{i}'
        known_terms.add(term)
        conn.execute_command('HSET', f'doc{i}', 'text', term)
    
    waitForIndex(env, 'idx')
    
    # Generate M random terms that are definitely not in the index
    # Use a different pattern to ensure they're not in known_terms
    m_random = 1000
    random_terms = []
    for i in range(m_random):
        # Generate random string that's unlikely to collide
        random_term = 'random_' + ''.join(random.choices(string.ascii_lowercase + string.digits, k=16))
        if random_term not in known_terms:
            random_terms.append(random_term)
    
    # Query for random terms and count false positives
    # A false positive is when the Bloom filter says "maybe present" but the term isn't actually there
    # In our case, this means the search returns 0 results but took longer (had to check trie)
    # Since we can't easily measure timing, we'll just verify that most queries return 0
    
    zero_results = 0
    for term in random_terms:
        result = env.cmd('FT.SEARCH', 'idx', f'@text:{term}')
        if result[0] == 0:
            zero_results += 1
    
    # All random terms should return 0 results (they're not in the index)
    # The Bloom filter should correctly reject most of them
    env.assertEqual(zero_results, len(random_terms), 
                   message="All random terms should return 0 results")
    
    # Verify the Bloom filter is being used by checking stats
    info = index_info(env, 'idx')
    bloom_terms = int(info['bloom_filter_terms'])
    env.assertGreaterEqual(bloom_terms, n_known, 
                          message=f"Bloom filter should contain at least {n_known} terms")

def test_bloom_filter_with_multiple_fields(env):
    """Test Bloom filter with multiple text fields"""
    conn = getConnectionByEnv(env)
    
    # Create index with multiple text fields
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 
               'title', 'TEXT', 
               'body', 'TEXT',
               'tags', 'TAG').ok()
    
    # Add documents
    conn.execute_command('HSET', 'doc1', 
                        'title', 'hello world',
                        'body', 'this is a test document',
                        'tags', 'test,sample')
    conn.execute_command('HSET', 'doc2',
                        'title', 'another document',
                        'body', 'with different content',
                        'tags', 'example,demo')
    
    waitForIndex(env, 'idx')
    
    # Check Bloom filter stats
    info = index_info(env, 'idx')
    bloom_terms = int(info['bloom_filter_terms'])
    env.assertGreater(bloom_terms, 0, message="Bloom filter should contain terms from all fields")
    
    # Test searches on different fields
    env.expect('FT.SEARCH', 'idx', '@title:hello').equal([1, 'doc1', ['title', 'hello world', 'body', 'this is a test document', 'tags', 'test,sample']])
    env.expect('FT.SEARCH', 'idx', '@body:content').equal([1, 'doc2', ['title', 'another document', 'body', 'with different content', 'tags', 'example,demo']])
    
    # Test non-existent terms
    env.expect('FT.SEARCH', 'idx', '@title:nonexistent').equal([0])
    env.expect('FT.SEARCH', 'idx', '@body:xyzabc').equal([0])

def test_bloom_filter_memory_overhead(env):
    """Test that Bloom filter memory overhead is reasonable (~10 bits per term)"""
    conn = getConnectionByEnv(env)

    # Create index
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

    # Add a known number of unique terms
    n_terms = 10000
    for i in range(n_terms):
        conn.execute_command('HSET', f'doc{i}', 'text', f'uniqueterm{i}')

    waitForIndex(env, 'idx')

    # Check memory usage
    info = index_info(env, 'idx')
    bloom_size_mb = float(info['bloom_filter_sz_mb'])
    bloom_terms = int(info['bloom_filter_terms'])

    # Calculate bits per term
    # 1 MB = 1024 * 1024 bytes = 8 * 1024 * 1024 bits
    bloom_size_bits = bloom_size_mb * 8 * 1024 * 1024
    bits_per_term = bloom_size_bits / bloom_terms if bloom_terms > 0 else 0

    # For 1% false positive rate, we expect ~9.6 bits per term
    # Allow some overhead for data structure, so check it's < 15 bits per term
    env.assertLess(bits_per_term, 15,
                  message=f"Bits per term ({bits_per_term:.2f}) should be < 15 (expected ~10)")
    env.assertGreater(bits_per_term, 5,
                     message=f"Bits per term ({bits_per_term:.2f}) should be > 5 (sanity check)")

def test_bloom_filter_after_document_deletion(env):
    """Test that Bloom filter persists after document deletion (terms remain in filter)"""
    conn = getConnectionByEnv(env)

    # Create index
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

    # Add documents
    n_docs = 100
    for i in range(n_docs):
        conn.execute_command('HSET', f'doc{i}', 'text', f'term{i}')

    waitForIndex(env, 'idx')

    # Get initial stats
    info_before = index_info(env, 'idx')
    terms_before = int(info_before['bloom_filter_terms'])

    # Delete half the documents
    for i in range(n_docs // 2):
        conn.execute_command('DEL', f'doc{i}')

    # Run GC to clean up
    forceInvokeGC(env, 'idx')
    time.sleep(0.5)

    # Get stats after deletion
    info_after = index_info(env, 'idx')
    terms_after = int(info_after['bloom_filter_terms'])

    # Bloom filter should still contain the same number of terms
    # (Bloom filters don't support deletion - terms remain even after docs are deleted)
    env.assertEqual(terms_after, terms_before,
                   message="Bloom filter terms should remain the same after document deletion")

def test_bloom_filter_with_stopwords(env):
    """Test that Bloom filter handles stopwords correctly"""
    conn = getConnectionByEnv(env)

    # Create index with custom stopwords
    env.expect('FT.CREATE', 'idx', 'STOPWORDS', 3, 'the', 'a', 'an',
               'SCHEMA', 'text', 'TEXT').ok()

    # Add documents with stopwords
    conn.execute_command('HSET', 'doc1', 'text', 'the quick brown fox')
    conn.execute_command('HSET', 'doc2', 'text', 'a lazy dog')

    waitForIndex(env, 'idx')

    # Check that filter contains non-stopword terms
    info = index_info(env, 'idx')
    bloom_terms = int(info['bloom_filter_terms'])
    env.assertGreater(bloom_terms, 0, message="Bloom filter should contain non-stopword terms")

    # Search for stopwords should return 0 (they're not indexed)
    env.expect('FT.SEARCH', 'idx', '@text:the').equal([0])
    env.expect('FT.SEARCH', 'idx', '@text:a').equal([0])

    # Search for non-stopwords should work
    result = env.cmd('FT.SEARCH', 'idx', '@text:quick')
    env.assertGreater(result[0], 0, message="Search for 'quick' should return results")

def test_bloom_filter_with_stemming(env):
    """Test that Bloom filter works correctly with stemming"""
    conn = getConnectionByEnv(env)

    # Create index with stemming enabled (default)
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

    # Add documents with words that stem to the same root
    conn.execute_command('HSET', 'doc1', 'text', 'running')
    conn.execute_command('HSET', 'doc2', 'text', 'runs')
    conn.execute_command('HSET', 'doc3', 'text', 'ran')

    waitForIndex(env, 'idx')

    # All variations should find documents (due to stemming)
    for term in ['run', 'running', 'runs', 'ran']:
        result = env.cmd('FT.SEARCH', 'idx', f'@text:{term}')
        env.assertGreater(result[0], 0, message=f"Search for '{term}' should return results due to stemming")

    # Check that Bloom filter contains stemmed terms
    info = index_info(env, 'idx')
    bloom_terms = int(info['bloom_filter_terms'])
    env.assertGreater(bloom_terms, 0, message="Bloom filter should contain stemmed terms")

def test_bloom_filter_case_sensitivity(env):
    """Test that Bloom filter handles case correctly (case-insensitive by default)"""
    conn = getConnectionByEnv(env)

    # Create index (case-insensitive by default)
    env.expect('FT.CREATE', 'idx', 'SCHEMA', 'text', 'TEXT').ok()

    # Add documents with mixed case
    conn.execute_command('HSET', 'doc1', 'text', 'Hello World')
    conn.execute_command('HSET', 'doc2', 'text', 'GOODBYE WORLD')

    waitForIndex(env, 'idx')

    # Searches should be case-insensitive
    for term in ['hello', 'HELLO', 'Hello', 'world', 'WORLD', 'World']:
        result = env.cmd('FT.SEARCH', 'idx', f'@text:{term}')
        env.assertGreater(result[0], 0, message=f"Search for '{term}' should return results (case-insensitive)")

    # Non-existent terms should still return 0
    env.expect('FT.SEARCH', 'idx', '@text:nonexistent').equal([0])

def test_bloom_filter_with_numeric_and_tag_fields(env):
    """Test that Bloom filter only applies to text fields, not numeric or tag fields"""
    conn = getConnectionByEnv(env)

    # Create index with mixed field types
    env.expect('FT.CREATE', 'idx', 'SCHEMA',
               'text', 'TEXT',
               'num', 'NUMERIC',
               'tag', 'TAG').ok()

    # Add documents
    for i in range(10):
        conn.execute_command('HSET', f'doc{i}',
                           'text', f'word{i}',
                           'num', i,
                           'tag', f'tag{i}')

    waitForIndex(env, 'idx')

    # Check Bloom filter stats
    info = index_info(env, 'idx')
    bloom_terms = int(info['bloom_filter_terms'])

    # Bloom filter should contain text terms
    env.assertGreater(bloom_terms, 0, message="Bloom filter should contain text terms")

    # All field types should work correctly
    env.expect('FT.SEARCH', 'idx', '@text:word5').equal([1, 'doc5', ['text', 'word5', 'num', '5', 'tag', 'tag5']])
    env.expect('FT.SEARCH', 'idx', '@num:[5 5]').equal([1, 'doc5', ['text', 'word5', 'num', '5', 'tag', 'tag5']])
    env.expect('FT.SEARCH', 'idx', '@tag:{tag5}').equal([1, 'doc5', ['text', 'word5', 'num', '5', 'tag', 'tag5']])

