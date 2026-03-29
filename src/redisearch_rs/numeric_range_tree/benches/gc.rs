/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

// Link both Rust-provided and C-provided symbols
extern crate redisearch_rs;
// Mock or stub the ones that aren't provided by the line above
redis_mock::mock_or_stub_missing_redis_c_symbols!();

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use inverted_index::NumericFilter;
use numeric_range_tree::{
    NumericRangeTree,
    test_utils::{DEEP_TREE_ENTRIES, SPLIT_TRIGGER, build_tree, gc_all_ranges},
};

#[derive(Clone, Copy)]
enum Scenario {
    Sparse,
    SemiSparse,
}

impl Scenario {
    const fn name(self) -> &'static str {
        match self {
            Self::Sparse => "Sparse",
            Self::SemiSparse => "SemiSparse",
        }
    }

    fn setup_tree(self, n: u64) -> NumericRangeTree {
        let mut tree = build_tree(n, false, 0);
        match self {
            Self::Sparse => gc_all_ranges(&mut tree, &|doc_id| doc_id > n * 7 / 8),
            Self::SemiSparse => {
                gc_all_ranges(&mut tree, &|doc_id| doc_id <= n / 8 || doc_id > n * 7 / 8)
            }
        }
        tree
    }

    fn query_filter(self, n: u64) -> NumericFilter {
        match self {
            Self::Sparse => NumericFilter {
                min: (n * 7 / 8) as f64,
                max: n as f64,
                ..Default::default()
            },
            Self::SemiSparse => NumericFilter {
                min: 1.0,
                max: (n / 8) as f64,
                ..Default::default()
            },
        }
    }
}

fn benchmark_gc(c: &mut Criterion) {
    let mut group = c.benchmark_group("GC");
    group.measurement_time(Duration::from_secs(10));
    group.warm_up_time(Duration::from_millis(500));

    for scenario in [Scenario::Sparse, Scenario::SemiSparse] {
        for n in [SPLIT_TRIGGER * 2, 5_000, DEEP_TREE_ENTRIES] {
            let setup = || scenario.setup_tree(n);
            {
                let mut tree = setup();
                assert!(
                    tree.is_sparse(),
                    "{} / {n}: tree should be sparse after GC",
                    scenario.name()
                );
                let leaves_before = tree.num_leaves();
                let memory_before = tree.mem_usage();
                tree.compact_if_sparse();
                assert!(
                    tree.mem_usage() <= memory_before,
                    "{} / {n}: compaction should not grow memory",
                    scenario.name()
                );
                assert!(
                    tree.num_leaves() <= leaves_before,
                    "{} / {n}: compaction should not increase leaves",
                    scenario.name()
                );
                assert!(
                    tree.bytes_reclaimed() > 0,
                    "{} / {n}: benchmark setup must reclaim bytes",
                    scenario.name()
                );
            }

            group.bench_with_input(
                BenchmarkId::new(format!("Compact/{}", scenario.name()), n),
                &n,
                |b, _| {
                    b.iter_batched(
                        setup,
                        |mut tree| tree.compact_if_sparse(),
                        BatchSize::SmallInput,
                    )
                },
            );

            let mut compacted_tree = setup();
            compacted_tree.compact_if_sparse();
            let filter = scenario.query_filter(n);
            group.bench_function(
                BenchmarkId::new(format!("QueryAfterCompact/{}", scenario.name()), n),
                |b| b.iter(|| black_box(compacted_tree.find(black_box(&filter)))),
            );
        }
    }

    group.finish();
}

criterion_group!(benches, benchmark_gc);
criterion_main!(benches);
