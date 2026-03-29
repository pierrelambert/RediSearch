/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Property-based tests for the numeric range tree using `proptest`.

#[cfg(not(miri))]
mod proptests {
    use std::collections::BTreeSet;

    use inverted_index::{IndexReader, NumericFilter, RSIndexResult};
    use numeric_range_tree::NumericRangeTree;
    use proptest::{prop_assert, prop_assert_eq, prop_assume};

    use numeric_range_tree::test_utils::gc_all_ranges;

    fn collect_matching_doc_ids(
        tree: &NumericRangeTree,
        filter: &NumericFilter,
        values_by_doc_id: &[(u64, f64)],
    ) -> Vec<u64> {
        let mut actual = BTreeSet::new();
        for range in tree.find(filter) {
            let mut reader = range.reader();
            let mut result = RSIndexResult::build_numeric(0.0).build();
            while reader.next_record(&mut result).unwrap_or(false) {
                // SAFETY: Numeric tree ranges always yield numeric index results.
                let value = unsafe { result.as_numeric_unchecked() };
                if filter.value_in_range(value) {
                    actual.insert(result.doc_id);
                }
            }
        }

        let expected: BTreeSet<u64> = values_by_doc_id
            .iter()
            .filter_map(|(doc_id, value)| filter.value_in_range(*value).then_some(*doc_id))
            .collect();

        assert_eq!(actual, expected);
        actual.into_iter().collect()
    }

    proptest::proptest! {
        #[test]
        fn prop_add_never_loses_entries(
            // Generate 1..200 doc/value pairs
            values in proptest::collection::vec((1u64..10000, -1000.0f64..1000.0), 1..200)
        ) {
            let mut tree = NumericRangeTree::new(false);
            let mut unique_ids = std::collections::HashSet::new();
            let mut last_id = 0u64;

            for (raw_id, value) in &values {
                // Ensure strictly increasing doc IDs.
                let doc_id = last_id + (*raw_id).max(1);
                last_id = doc_id;
                tree.add(doc_id, *value, false, 0);
                unique_ids.insert(doc_id);
            }

            assert_eq!(tree.num_entries(), unique_ids.len());
        }

        #[test]
        fn prop_find_returns_overlapping_ranges(
            // Build tree from random entries, then query with random filter
            values in proptest::collection::vec(-1000.0f64..1000.0, 1..100),
            filter_min in -1500.0f64..1500.0,
            filter_width in 0.0f64..3000.0,
        ) {
            let mut tree = NumericRangeTree::new(false);
            for (i, value) in values.iter().enumerate() {
                tree.add((i + 1) as u64, *value, false, 0);
            }

            let filter_max = filter_min + filter_width;
            let filter = NumericFilter {
                min: filter_min,
                max: filter_max,
                ..Default::default()
            };
            let ranges = tree.find(&filter);

            for range in &ranges {
                assert!(
                    range.overlaps(filter_min, filter_max),
                    "range [{}, {}] does not overlap filter [{filter_min}, {filter_max}]",
                    range.min_val(),
                    range.max_val(),
                );
            }

        }

        #[test]
        fn prop_tree_invariants_after_operations(
            values in proptest::collection::vec(-5000.0f64..5000.0, 1..300)
        ) {
            let mut tree = NumericRangeTree::new(false);
            // The depth imbalance invariant in `check_tree_invariants` (which
            // runs after every `add` under the `unittest` feature) validates
            // balance at every node automatically.
            for (i, value) in values.iter().enumerate() {
                tree.add((i + 1) as u64, *value, false, 0);
            }
        }

        #[test]
        fn prop_find_ascending_descending_same_ranges(
            values in proptest::collection::vec(-1000.0f64..1000.0, 1..100),
            filter_min in -1500.0f64..1500.0,
            filter_width in 0.0f64..3000.0,
        ) {
            let mut tree = NumericRangeTree::new(false);
            for (i, value) in values.iter().enumerate() {
                tree.add((i + 1) as u64, *value, false, 0);
            }

            let filter_max = filter_min + filter_width;
            let filter_asc = NumericFilter {
                min: filter_min,
                max: filter_max,
                ascending: true,
                ..Default::default()
            };
            let filter_desc = NumericFilter {
                min: filter_min,
                max: filter_max,
                ascending: false,
                ..Default::default()
            };

            let ranges_asc = tree.find(&filter_asc);
            let ranges_desc = tree.find(&filter_desc);

            assert_eq!(
                ranges_asc.len(),
                ranges_desc.len(),
                "asc and desc should return the same number of ranges"
            );

            // Compare as sets by sorting on bit representation of bounds.
            let mut asc_ids: Vec<(u64, u64)> = ranges_asc
                .iter()
                .map(|r| (r.min_val().to_bits(), r.max_val().to_bits()))
                .collect();
            let mut desc_ids: Vec<(u64, u64)> = ranges_desc
                .iter()
                .map(|r| (r.min_val().to_bits(), r.max_val().to_bits()))
                .collect();
            asc_ids.sort();
            desc_ids.sort();
            assert_eq!(asc_ids, desc_ids);
        }

        #[test]
        fn prop_memory_usage_monotonic_with_adds(
            values in proptest::collection::vec(-1000.0f64..1000.0, 1..100)
        ) {
            let mut tree = NumericRangeTree::new(false);
            let mut last_mem = tree.mem_usage();

            for (i, value) in values.iter().enumerate() {
                tree.add((i + 1) as u64, *value, false, 0);
                let current_mem = tree.mem_usage();
                assert!(
                    current_mem >= last_mem,
                    "mem_usage decreased from {last_mem} to {current_mem} after add"
                );
                last_mem = current_mem;
            }
        }

        #[test]
        fn prop_gc_then_trim_preserves_surviving_docs(
            num_entries in 10u64..200,
            delete_ratio in 0.1f64..0.9,
            max_depth_range in 0usize..3,
        ) {
            let mut tree = NumericRangeTree::new(false);
            for i in 1..=num_entries {
                // Use varied values to trigger splits.
                tree.add(i, (i % 50) as f64, false, max_depth_range);
            }

            let delete_threshold = (num_entries as f64 * delete_ratio) as u64;

            let surviving_count = num_entries - delete_threshold;

            // Apply GC to all ranges (leaves + retained internal ranges).
            gc_all_ranges(&mut tree, &|doc_id| doc_id > delete_threshold);

            // Trim empty leaves.
            tree.trim_empty_leaves();

            // num_entries should equal surviving count.
            assert_eq!(
                tree.num_entries(),
                surviving_count as usize,
                "after GC + trim, num_entries should be {surviving_count}"
            );
        }

        #[test]
        fn prop_compaction_preserves_range_queries(
            values in proptest::collection::vec(-20_000i32..20_000, 64..256),
            queries in proptest::collection::vec((-25_000i32..25_000, 0u16..20_000, proptest::bool::ANY), 8..24),
        ) {
            let mut tree = NumericRangeTree::new(false);
            let doc_values: Vec<(u64, f64)> = values
                .iter()
                .enumerate()
                .map(|(idx, value)| ((idx + 1) as u64, *value as f64))
                .collect();
            for (doc_id, value) in &doc_values {
                tree.add(*doc_id, *value, false, 0);
            }

            let mut sorted_values: Vec<f64> = doc_values.iter().map(|(_, value)| *value).collect();
            sorted_values.sort_by(f64::total_cmp);
            let keep_threshold = sorted_values[sorted_values.len() * 3 / 4];

            gc_all_ranges(&mut tree, &|doc_id| {
                doc_values[(doc_id - 1) as usize].1 >= keep_threshold
            });

            let surviving_values: Vec<(u64, f64)> = doc_values
                .iter()
                .copied()
                .filter(|(_, value)| *value >= keep_threshold)
                .collect();
            prop_assume!(!surviving_values.is_empty());
            prop_assume!(tree.is_sparse());

            let mut before_results = Vec::with_capacity(queries.len());
            for (raw_min, raw_width, ascending) in &queries {
                let max = *raw_min as f64 + *raw_width as f64;
                let filter = NumericFilter {
                    min: *raw_min as f64,
                    max,
                    ascending: *ascending,
                    ..Default::default()
                };
                before_results.push(collect_matching_doc_ids(&tree, &filter, &surviving_values));
            }

            let bytes_before = tree.bytes_reclaimed();
            let runs_before = tree.compaction_runs();
            tree.compact_if_sparse();
            prop_assert_eq!(tree.compaction_runs(), runs_before + 1);
            prop_assert!(tree.bytes_reclaimed() >= bytes_before);

            for ((raw_min, raw_width, ascending), before_result) in queries.iter().zip(before_results) {
                let filter = NumericFilter {
                    min: *raw_min as f64,
                    max: *raw_min as f64 + *raw_width as f64,
                    ascending: *ascending,
                    ..Default::default()
                };
                let after_result = collect_matching_doc_ids(&tree, &filter, &surviving_values);
                prop_assert_eq!(before_result, after_result);
            }
        }
    }
}
