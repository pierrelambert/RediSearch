# SQL Benchmarks

This directory contains the SQL semantic-layer benchmark set used for item 7
closure. It covers both translation microbenchmarks in Rust and end-to-end
module benchmarks against native command baselines.

## Microbenchmarks

Location: [sql_translation.rs](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/src/redisearch_rs/sql_parser/benches/sql_translation.rs)

Run from the Rust workspace root:

```bash
cd src/redisearch_rs
cargo bench -p sql_parser --bench sql_translation
```

Coverage:
- uncached translation for plain search, aggregate, vector, and hybrid SQL
- hot-cache translation for the same query shapes
- schema-churn cache-miss behavior
- cache stats read overhead

The Criterion HTML report is written under
`bin/redisearch_rs/criterion/report/index.html`.

## End-To-End Benchmarks

Use `redisbench-admin` as documented in
[developer.md](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/developer.md).
Run one benchmark file at a time with `--test tests/benchmarks/<file>.yml`.

### Local status snapshot (2026-03-30)

- `redisbench-admin --version`: `redisearch 0.1.0`
- `memtier_benchmark`: present locally
- `ftsb_redisearch`: not present locally in this checkout
- SQL behavioral validation completed separately and passed:
  - `test_sql_layer.py`: `46/46`
  - runtime boolean config validation for `SQL_ENABLED`: passed
- Verified local dry-run on
  `vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql.yml`:
  Redis spun up, dataset preload completed, `FT.CONFIG SET SQL_ENABLED true`
  succeeded, index loading completed, and client connectivity checks passed.
- Limitation observed in this environment: the corresponding timed local run hit
  `redis.exceptions.BusyLoadingError` shortly after Redis became reachable and
  did not emit a benchmark JSON artifact. Treat this as a local-runner/env
  blocker, not as release-hardware evidence.

### Local microbenchmark snapshot (2026-03-30)

From the generated Criterion artifacts under `bin/redisearch_rs/criterion/`:

- uncached basic search: ~10.01 µs
- uncached aggregate/group-by: ~19.14 µs
- uncached vector KNN: ~9.26 µs
- uncached weighted hybrid: ~10.25 µs
- cached-hot basic search: ~624 ns
- cached-hot aggregate/group-by: ~1.13 µs
- cached-hot vector KNN: ~369 ns
- cached-hot weighted hybrid: ~1.20 µs
- schema-churn miss (basic search): ~12.53 µs
- schema-churn miss (hybrid): ~13.13 µs
- cache stats read: ~14.7 ns

Search/aggregate pairings:
- native search baseline:
  [search-ftsb-10K-enwiki_abstract-hashes-fulltext-search-sortby-limit-0-100.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-fulltext-search-sortby-limit-0-100.yml)
- SQL search variant:
  [search-ftsb-10K-enwiki_abstract-hashes-sql-search-sortby-limit-0-100.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-sql-search-sortby-limit-0-100.yml)
- native aggregate baseline:
  [search-ftsb-10K-enwiki_abstract-hashes-groupby-title-limit-0-100.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-groupby-title-limit-0-100.yml)
- SQL aggregate variant:
  [search-ftsb-10K-enwiki_abstract-hashes-sql-groupby-title-limit-0-100.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-sql-groupby-title-limit-0-100.yml)

Vector/hybrid pairings:
- native vector baseline on the SQL-supported surface:
  [vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql-surface.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql-surface.yml)
- SQL vector variant:
  [vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql.yml)
- native hybrid baseline on the SQL-supported surface:
  [vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql-surface.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql-surface.yml)
- SQL hybrid variant:
  [vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql.yml](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql.yml)

## Sign-Off Data To Capture

- uncached translation latency from the Criterion bench
- cache-hit latency from the Criterion bench
- cache-miss behavior under schema churn from the Criterion bench
- `FT.SQL` versus native baseline latency for each YAML pair
- memory notes for translation cache footprint and Criterion report size

## Release Evidence Template

Use this checklist when preparing the external release artifact for `FT.SQL`.
The branch can carry the benchmark recipes and this template, but the actual
release-hardware results and approval remain external sign-off data.

```text
Build SHA:
Build date:
Release owner:
Hardware / instance type:
Redis / module build flags:
redisbench-admin version:

Criterion microbenchmarks
- uncached translation:
- cache-hit translation:
- schema-churn cache miss:
- cache stats read:

End-to-end paired benchmarks
- native search baseline:
- SQL search variant:
- native aggregate baseline:
- SQL aggregate variant:
- native vector baseline:
- SQL vector variant:
- native hybrid baseline:
- SQL hybrid variant:

Observed deltas / notes:
Rollback path verified:
Default-on approved:
```

## Recommended Release Gate

- no pathological regression between SQL and native baselines for the paired
  benchmark files
- cache-hit translation stays within the design target envelope
- hybrid SQL does not regress materially beyond the native `FT.HYBRID`
  baseline on the same dataset and query shape
- release-hardware numbers are captured outside the repo and signed by the
  release owner before any default-on decision
