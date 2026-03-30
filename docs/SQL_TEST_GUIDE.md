# SQL Semantic Layer - Verified Test Guide

## Status

`FT.SQL` is still an experimental/beta feature in this branch and remains gated
by `SQL_ENABLED=false` by default.

- Authoritative user-facing contract: [FT.SQL command reference](commands/FT.SQL.md)
- Architecture and rollout notes: [SQL semantic layer design](SQL_SEMANTIC_LAYER_DESIGN.md)
- Behavioral coverage: [tests/pytests/test_sql_layer.py](../tests/pytests/test_sql_layer.py)
- Remaining open closure item: release gate/default-on sign-off (`item 8`)

This guide documents only examples that map to module-level behavioral tests.
It intentionally omits older exploratory examples that were broader than the
audited branch surface.

## Local Verification Snapshot (2026-03-30)

- Conclusion: `FT.SQL` is technically cleaner and locally well verified in-repo,
  but it is **not** ready for GA/default-on in this branch.
- Runtime status remains unchanged: experimental/beta and guarded by
  `SQL_ENABLED=false` by default.
- Verified locally in-repo:
  - Rust translator regression coverage for boolean precedence, including
    `(A OR B) AND NOT C` composition.
  - Python behavioral coverage for the SQL surface, including the default-off
    gate and runtime disable path.
  - Criterion translation microbenchmarks with reports under
    `bin/redisearch_rs/criterion/`.
- Concrete local results captured on 2026-03-30:
  - `./build.sh RUN_PYTEST TEST=test_sql_layer.py`: passed (`46/46`)
  - `./build.sh RUN_PYTEST TEST=test_config.py::testConfigAPIRunTimeBooleanParams`:
    passed
  - targeted Rust translator regression for boolean precedence: passed
  - translation microbenchmarks completed and produced Criterion reports
- Still open before any default-on decision:
  - release-hardware paired benchmark results for the SQL/native YAML pairs
  - release-owner sign-off on the default-on change
  - full-text benchmark pair execution in an environment that provides
    `ftsb_redisearch`
  - stable timed local `redisbench-admin` evidence for the vector/hybrid pair in
    this macOS checkout; dry-run and preload succeeded, but the timed run hit a
    `BusyLoadingError` before producing a result artifact

## Local Benchmark Results (2026-03-30)

Criterion translation microbenchmarks:

| Scenario | Mean |
|----------|------|
| uncached basic search | ~10.01 µs |
| uncached aggregate/group-by | ~19.14 µs |
| uncached vector KNN | ~9.26 µs |
| uncached weighted hybrid | ~10.25 µs |
| cached-hot basic search | ~624 ns |
| cached-hot aggregate/group-by | ~1.13 µs |
| cached-hot vector KNN | ~369 ns |
| cached-hot weighted hybrid | ~1.20 µs |
| schema-churn miss (basic search) | ~12.53 µs |
| schema-churn miss (hybrid) | ~13.13 µs |
| cache stats read | ~14.7 ns |

Local end-to-end benchmark status:

- `redisbench-admin` dry-run with preload for the SQL vector benchmark:
  succeeded (Redis start, dataset preload, `SQL_ENABLED=true`, index load, and
  connectivity checks all passed)
- timed local SQL vector run: did not complete successfully in this checkout;
  failed with `redis.exceptions.BusyLoadingError` and produced no result JSON

## Verified Surface

| Surface | Current branch status | Backend |
|---------|-----------------------|---------|
| `SELECT *`, projection, aliases, `LIMIT/OFFSET` | Verified | `FT.SEARCH` |
| Equality on `TAG`, numeric comparisons, `BETWEEN` | Verified | `FT.SEARCH` |
| `IN` / `NOT IN` | Verified | `FT.SEARCH` |
| `LIKE` / `NOT LIKE` | Verified | `FT.SEARCH` |
| `IS NULL` / `IS NOT NULL` on `INDEXMISSING` fields | Verified | `FT.SEARCH` |
| Boolean composition with `AND` / `OR` / `NOT` | Verified | `FT.SEARCH` |
| Single-column `ORDER BY` on plain queries | Verified | `FT.SEARCH` |
| `GROUP BY`, aggregate aliases, `HAVING` | Verified | `FT.AGGREGATE` |
| Multi-column `ORDER BY` on aggregate queries | Verified | `FT.AGGREGATE` |
| pgvector-style KNN (`<->`) | Verified | `FT.SEARCH` |
| Weighted Hybrid via `OPTION (vector_weight, text_weight)` | Verified | `FT.HYBRID` |
| JSON indexes with aliased fields | Verified | `FT.SEARCH` |
| Practical RediSearch index names such as `idx:all` | Verified | Parser + public command path |
| Coordinator/public path search and aggregate parity | Verified | Public coordinator path |
| Coordinator/public path vector, Hybrid, and JSON coverage | Verified behavioral coverage | Public coordinator path |
| Advanced Hybrid knobs beyond weights | Not supported | N/A |
| Multi-column `ORDER BY` on plain search queries | Rejected | N/A |
| Dedicated SQL full-text predicates, joins, subqueries, `UNION`, geo SQL | Not supported | N/A |

## Enable The Feature

```bash
redis-cli FT.CONFIG SET SQL_ENABLED true
```

The disabled-by-default gate and runtime disable behavior are covered by
`test_sql_disabled_by_default` and `test_sql_runtime_disable_blocks_queries`.

## Minimal Setup

```bash
redis-cli FT.CREATE products SCHEMA \
  name TEXT SORTABLE \
  category TAG \
  price NUMERIC SORTABLE \
  stock NUMERIC \
  embedding VECTOR FLAT 6 TYPE FLOAT32 DIM 2 DISTANCE_METRIC L2

redis-cli HSET prod:1 name "Laptop" category "electronics" price 1000 stock 50
redis-cli HSET prod:2 name "Phone" category "electronics" price 500 stock 200
redis-cli HSET prod:3 name "Desk" category "furniture" price 200 stock 20
```

For vector and Hybrid queries, the documents must also contain binary vector
payloads in the indexed vector field. The SQL query still uses a vector literal
such as `'[0.0, 0.0]'`; RediSearch stores the document embeddings as binary.

## Verified Examples

### Core Search

```bash
redis-cli FT.SQL "SELECT * FROM products"
redis-cli FT.SQL "SELECT name, price FROM products"
redis-cli FT.SQL "SELECT name, stock FROM products WHERE price > 50"
redis-cli FT.SQL "SELECT * FROM products WHERE price BETWEEN 50 AND 200"
redis-cli FT.SQL "SELECT * FROM products ORDER BY price DESC LIMIT 2 OFFSET 1"
redis-cli FT.SQL "SELECT category, price FROM idx:all WHERE category = 'electronics' ORDER BY price ASC LIMIT 1"
```

Backed by:
`test_sql_select_star`, `test_sql_select_fields`,
`test_sql_select_fields_with_where`, `test_sql_where_between`,
`test_sql_order_by_desc`, `test_sql_limit_offset`, and
`test_sql_search_parity_with_colon_index_name`.

### Predicate Surface

```bash
redis-cli FT.SQL \
  "SELECT category, price FROM products \
   WHERE category IN ('electronics', 'accessories') ORDER BY price ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT category, price FROM products \
   WHERE category NOT IN ('clearance', 'furniture') ORDER BY price ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT name FROM products \
   WHERE name LIKE '%top' ORDER BY name ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT name FROM products \
   WHERE name NOT LIKE '%top' ORDER BY name ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT title, rank FROM products \
   WHERE nickname IS NULL ORDER BY rank ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT title, rank FROM products \
   WHERE nickname IS NOT NULL ORDER BY rank ASC LIMIT 10"

redis-cli FT.SQL \
  "SELECT category, price FROM products \
   WHERE (category = 'electronics' OR category = 'accessories') \
   AND NOT (status = 'archived') ORDER BY price ASC LIMIT 10"
```

Backed by:
`test_sql_in_parity`, `test_sql_not_in_parity`, `test_sql_like_parity`,
`test_sql_not_like_parity`, `test_sql_is_null_parity`,
`test_sql_is_not_null_parity`, and `test_sql_boolean_composition_parity`.

Leading-wildcard `LIKE` examples depend on underlying RediSearch field options
such as `WITHSUFFIXTRIE`. The behavioral tests use wildcard-capable TEXT fields
for those cases.

### Aggregation

```bash
redis-cli FT.SQL \
  "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price \
   FROM products GROUP BY category HAVING COUNT(*) >= 2 ORDER BY category ASC"

redis-cli FT.SQL \
  "SELECT category, COUNT(*) AS cnt \
   FROM products GROUP BY category ORDER BY cnt DESC, category ASC"
```

Backed by:
`test_sql_group_by_having_parity` and
`test_sql_group_by_multi_order_by_parity`.

### Vector KNN

```bash
redis-cli FT.SQL \
  "SELECT category FROM products \
   WHERE category = 'electronics' \
   ORDER BY embedding <-> '[0.1, 0.0]' LIMIT 2"
```

This routes to `FT.SEARCH` KNN syntax internally. Backed by
`test_sql_vector_knn_query`.

### Weighted Hybrid

```bash
redis-cli FT.SQL \
  "SELECT * FROM products \
   WHERE category = 'electronics' \
   ORDER BY embedding <-> '[0.0, 0.0]' LIMIT 2 \
   OPTION (vector_weight = 0.7, text_weight = 0.3)"
```

This routes to the live `FT.HYBRID SEARCH ... VSIM ... COMBINE LINEAR ... PARAMS`
grammar. Backed by `test_sql_hybrid_query`.

Defaulting the omitted weight to `0.5` is covered by
`test_sql_hybrid_query_default_text_weight`.

### JSON Indexes

```bash
redis-cli FT.CREATE products_json ON JSON PREFIX 1 product: SCHEMA \
  $.name AS name TEXT \
  $.price AS price NUMERIC SORTABLE \
  $.category AS category TAG

redis-cli FT.SQL \
  "SELECT name, price FROM products_json \
   WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
```

Backed by `test_sql_json_index_search_parity`.

### Coordinator / Cluster Coverage

Use the same SQL syntax in cluster mode. The coordinator/public path is covered
by:

- `test_sql_cluster_search_parity`
- `test_sql_cluster_aggregate_parity`
- `test_sql_cluster_vector_knn_parity`
- `test_sql_cluster_hybrid_parity`
- `test_sql_cluster_json_index_search_parity`

## Rejected And Unsupported Forms

The following forms intentionally error out in the current branch:

```bash
# Plain search queries cannot sort by multiple columns.
redis-cli FT.SQL "SELECT * FROM products ORDER BY price DESC, name ASC"

# OPTION(...) is only valid for weighted vector+text Hybrid queries.
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics' OPTION (vector_weight = 0.7)"

# TEXT equality is rejected because SQL v1 does not expose dedicated full-text predicates.
redis-cli FT.SQL "SELECT * FROM products WHERE name = 'Laptop'"
```

Backed by:
`test_sql_multi_order_by_plain_search_is_rejected`,
`test_sql_option_without_vector_is_rejected`, and
`test_sql_text_equality_is_rejected`.

## Validation Guards

The current validator rejects overly large or structurally expensive queries:

- Maximum SQL string length: `65536` bytes
- Maximum condition nesting depth: `32`
- Maximum `IN` list size: `1000`
- Maximum projected columns: `100`
- Maximum `ORDER BY` columns: `8`

These guards are enforced before execution in the Rust validation layer.

## Notes

- `FT.SQL` accepts exactly one SQL string argument.
- The response shape follows the translated backend command:
  `FT.SEARCH`, `FT.AGGREGATE`, or `FT.HYBRID`.
- `IS NULL` / `IS NOT NULL` depend on the underlying field being indexed with
  `INDEXMISSING`.
- `FT.SQL` remains experimental/beta and default-off in this branch. The
  external release gate for benchmark evidence and release-owner approval is
  still open.
