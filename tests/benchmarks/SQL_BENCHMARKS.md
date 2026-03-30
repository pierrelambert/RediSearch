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

## Recommended Release Gate

- no pathological regression between SQL and native baselines for the paired
  benchmark files
- cache-hit translation stays within the design target envelope
- hybrid SQL does not regress materially beyond the native `FT.HYBRID`
  baseline on the same dataset and query shape
