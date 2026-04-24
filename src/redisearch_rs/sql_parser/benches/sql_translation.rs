/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use sql_parser::{
    CacheStats, FieldCapabilities, QuerySchema, clear_cache, get_cache_stats,
    translate_cached_with_schema, translate_with_schema,
};

const BASIC_SEARCH_SQL: &str =
    "SELECT name, price FROM products WHERE category = 'electronics' ORDER BY price DESC LIMIT 25";
const AGGREGATE_SQL: &str = "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price \
    FROM products GROUP BY category HAVING COUNT(*) >= 2 ORDER BY category ASC";
const VECTOR_SQL: &str = "SELECT * FROM products WHERE labels = 'stat.ME' \
    ORDER BY vector <=> '[0.7470588088,0.7470588088,0.7470588088,0.7470588088]' LIMIT 25";
const HYBRID_SQL: &str = "SELECT * FROM products WHERE labels = 'stat.ME' \
    ORDER BY vector <=> '[0.7470588088,0.7470588088,0.7470588088,0.7470588088]' LIMIT 25 \
    OPTION (vector_weight = 0.7, text_weight = 0.3)";

fn base_schema(version: u64) -> QuerySchema {
    QuerySchema::new(version)
        .with_field("category", FieldCapabilities::tag())
        .with_field("labels", FieldCapabilities::tag())
        .with_field("name", FieldCapabilities::text())
        .with_field("title", FieldCapabilities::text())
        .with_field("abstract", FieldCapabilities::text())
}

fn translation_cases() -> [(&'static str, &'static str); 4] {
    [
        ("basic_search", BASIC_SEARCH_SQL),
        ("aggregate_group_by", AGGREGATE_SQL),
        ("vector_knn", VECTOR_SQL),
        ("hybrid_weighted", HYBRID_SQL),
    ]
}

fn benchmark_uncached_translation(c: &mut Criterion) {
    let schema = base_schema(7);
    let mut group = c.benchmark_group("sql_translate_uncached");

    for (name, sql) in translation_cases() {
        group.bench_function(name, |b| {
            b.iter(|| translate_with_schema(black_box(sql), black_box(&schema)).unwrap())
        });
    }

    group.finish();
}

fn benchmark_cached_hot_translation(c: &mut Criterion) {
    let schema = base_schema(7);
    let mut group = c.benchmark_group("sql_translate_cached_hot");

    for (name, sql) in translation_cases() {
        group.bench_function(name, |b| {
            clear_cache();
            translate_cached_with_schema(sql, &schema).unwrap();
            let warm_stats = get_cache_stats();
            assert_eq!(warm_stats.hits, 0);
            assert_eq!(warm_stats.misses, 1);

            b.iter(|| translate_cached_with_schema(black_box(sql), black_box(&schema)).unwrap());

            let hot_stats = get_cache_stats();
            assert!(hot_stats.hits > 0, "expected cache hits for {name}");
        });
    }

    group.finish();
}

fn benchmark_cached_miss_translation(c: &mut Criterion) {
    let mut group = c.benchmark_group("sql_translate_cached_miss");

    for (name, sql) in [
        ("basic_search_schema_churn", BASIC_SEARCH_SQL),
        ("hybrid_schema_churn", HYBRID_SQL),
    ] {
        group.bench_function(name, |b| {
            clear_cache();
            let mut version = 100_u64;

            b.iter_batched(
                || {
                    version += 1;
                    base_schema(version)
                },
                |schema| {
                    translate_cached_with_schema(black_box(sql), black_box(&schema)).unwrap();
                },
                BatchSize::SmallInput,
            );

            let stats = get_cache_stats();
            assert_eq!(
                stats.hits, 0,
                "schema churn benchmark should force cache misses for {name}"
            );
            assert!(stats.misses > 0, "expected cache misses for {name}");
        });
    }

    group.finish();
}

fn benchmark_cache_stats_read(c: &mut Criterion) {
    clear_cache();
    let schema = base_schema(7);
    translate_cached_with_schema(BASIC_SEARCH_SQL, &schema).unwrap();
    translate_cached_with_schema(BASIC_SEARCH_SQL, &schema).unwrap();

    c.bench_function("sql_translate_cache_stats_read", |b| {
        b.iter(|| black_box(get_cache_stats()))
    });

    let stats: CacheStats = get_cache_stats();
    assert!(stats.hits > 0);
    assert!(stats.hit_rate() > 0.0);
}

criterion_group!(
    benches,
    benchmark_uncached_translation,
    benchmark_cached_hot_translation,
    benchmark_cached_miss_translation,
    benchmark_cache_stats_read
);
criterion_main!(benches);
