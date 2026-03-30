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

## Verified Surface

| Surface | Current branch status | Backend |
|---------|-----------------------|---------|
| `SELECT *`, projection, aliases, `LIMIT/OFFSET` | Verified | `FT.SEARCH` |
| Equality on `TAG`, numeric comparisons, `BETWEEN` | Verified | `FT.SEARCH` |
| Single-column `ORDER BY` on plain queries | Verified | `FT.SEARCH` |
| `GROUP BY`, aggregate aliases, `HAVING` | Verified | `FT.AGGREGATE` |
| Multi-column `ORDER BY` on aggregate queries | Verified | `FT.AGGREGATE` |
| pgvector-style KNN (`<->`) | Verified | `FT.SEARCH` |
| Weighted Hybrid via `OPTION (vector_weight, text_weight)` | Verified | `FT.HYBRID` |
| JSON indexes with aliased fields | Verified | `FT.SEARCH` |
| Coordinator/public `FT.SQL` registration | Verified smoke test | Public coordinator path |
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
```

Backed by:
`test_sql_select_star`, `test_sql_select_fields`,
`test_sql_select_fields_with_where`, `test_sql_where_between`,
`test_sql_order_by_desc`, and `test_sql_limit_offset`.

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

### Coordinator / Cluster Smoke

Use the same SQL syntax in cluster mode. The coordinator/public registration path
is covered by `test_sql_cluster_search_parity`.

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
- The broader SQL closure target still includes additional operators such as
  `IN` / `NOT IN`, `LIKE` / `NOT LIKE`, and boolean composition. Those remain
  tracked in the design doc, but this guide keeps its examples limited to the
  branch behaviors currently covered by module-level tests.
