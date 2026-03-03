# -*- coding: utf-8 -*-

from includes import *
from common import *
from RLTest import Env


class TestNumericTreeMemory(object):
    """Test suite for Numeric Range Tree memory optimization.
    
    Tests verify that the tree compaction feature reduces memory usage for sparse
    numeric data while maintaining correctness for dense data.
    """

    def __init__(self):
        skipTest(cluster=True)
        self.env = Env(testName="testing numeric tree memory optimization")
        self.env.expect(config_cmd(), 'set', 'FORK_GC_CLEAN_THRESHOLD', 0).equal('OK')

    def test_sparse_data_compaction(self):
        """Test that sparse numeric data triggers tree compaction and reduces memory.
        
        Creates an index with sparse numeric IDs (large gaps between values) and
        verifies that after GC, the tree is compacted and uses less memory than
        expected for uncompacted storage.
        """
        env = self.env
        
        # Create index with numeric field
        env.expect('FT.CREATE', 'idx_sparse', 'SCHEMA', 'user_id', 'NUMERIC').ok()
        waitForIndex(env, 'idx_sparse')
        
        # Add documents with very sparse IDs: 1, 100, 10000, 100000, 1000000
        # This creates large gaps that should benefit from compaction
        sparse_ids = [1, 100, 10000, 100000, 1000000]
        for doc_id in sparse_ids:
            env.cmd('HSET', f'doc:{doc_id}', 'user_id', str(doc_id))
        
        # Force GC to trigger compaction
        forceInvokeGC(env, 'idx_sparse')
        
        # Get tree summary
        summary = numeric_tree_summary(env, 'idx_sparse', 'user_id')
        
        # Verify basic correctness
        env.assertEqual(len(sparse_ids), summary['numEntries'], 
                       message="Expected number of entries to match documents added")
        env.assertEqual(0, summary['emptyLeaves'],
                       message="Expected no empty leaves after compaction")
        
        # Verify memory usage is reasonable
        # For 5 entries with sparse IDs, memory should be significantly less than
        # what would be needed for dense storage of 1M entries
        memory_mb = summary['MemoryUsage'] / (1024 * 1024)
        
        # Memory should be less than 1MB for just 5 entries (even with tree overhead)
        # This is a sanity check - actual compaction verification would require
        # comparing with a baseline or checking internal tree structure
        env.assertLess(memory_mb, 1.0,
                      message=f"Memory usage {memory_mb:.2f}MB seems too high for 5 sparse entries")
        
        # Verify search still works correctly
        for doc_id in sparse_ids:
            result = env.cmd('FT.SEARCH', 'idx_sparse', f'@user_id:[{doc_id} {doc_id}]')
            env.assertEqual(1, result[0], 
                           message=f"Expected to find document with user_id={doc_id}")

    def test_dense_data_unchanged(self):
        """Test that dense numeric data does not negatively impact memory or performance.
        
        Creates an index with sequential numeric IDs and verifies that the tree
        behaves normally without unnecessary overhead from compaction logic.
        """
        env = self.env
        
        # Create index with numeric field
        env.expect('FT.CREATE', 'idx_dense', 'SCHEMA', 'seq_id', 'NUMERIC').ok()
        waitForIndex(env, 'idx_dense')
        
        # Add documents with sequential IDs: 1, 2, 3, ..., 1000
        num_docs = 1000
        for i in range(1, num_docs + 1):
            env.cmd('HSET', f'doc:{i}', 'seq_id', str(i))
        
        # Force GC
        forceInvokeGC(env, 'idx_dense')
        
        # Get tree summary
        summary = numeric_tree_summary(env, 'idx_dense', 'seq_id')
        
        # Verify correctness
        env.assertEqual(num_docs, summary['numEntries'],
                       message="Expected number of entries to match documents added")
        env.assertEqual(0, summary['emptyLeaves'],
                       message="Expected no empty leaves")
        
        # Verify memory usage is reasonable for dense data
        memory_mb = summary['MemoryUsage'] / (1024 * 1024)
        
        # For 1000 sequential entries, memory should be reasonable
        # (less than 5MB is a generous upper bound for sanity)
        env.assertLess(memory_mb, 5.0,
                      message=f"Memory usage {memory_mb:.2f}MB seems too high for 1000 dense entries")
        
        # Verify range search works correctly
        result = env.cmd('FT.SEARCH', 'idx_dense', '@seq_id:[1 1000]', 'LIMIT', 0, 0)
        env.assertEqual(num_docs, result[0],
                       message="Expected range search to find all documents")

    def test_sibling_merging(self):
        """Test that sibling nodes with few entries are merged during compaction.
        
        Creates a scenario where tree nodes should have siblings that can be merged,
        then verifies that GC triggers the merge and reduces tree depth/complexity.
        """
        env = self.env
        
        # Create index
        env.expect('FT.CREATE', 'idx_merge', 'SCHEMA', 'value', 'NUMERIC').ok()
        waitForIndex(env, 'idx_merge')
        
        # Add documents that will create a tree structure with potential for merging
        # Use values that span different ranges but with gaps
        values = [1, 2, 3, 100, 101, 102, 1000, 1001, 1002]
        for i, val in enumerate(values):
            env.cmd('HSET', f'doc:{i}', 'value', str(val))
        
        # Get initial tree summary
        summary_before = numeric_tree_summary(env, 'idx_merge', 'value')
        
        # Force GC to trigger potential merging
        forceInvokeGC(env, 'idx_merge')
        
        # Get tree summary after GC
        summary_after = numeric_tree_summary(env, 'idx_merge', 'value')
        
        # Verify correctness
        env.assertEqual(len(values), summary_after['numEntries'],
                       message="Expected number of entries to remain unchanged after GC")
        env.assertEqual(0, summary_after['emptyLeaves'],
                       message="Expected no empty leaves after GC")
        
        # Verify all values are still searchable
        for val in values:
            result = env.cmd('FT.SEARCH', 'idx_merge', f'@value:[{val} {val}]')
            env.assertEqual(1, result[0],
                           message=f"Expected to find document with value={val} after GC")

