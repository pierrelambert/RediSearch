# FT.SQL

`FT.SQL` parses a SQL query string and dispatches it to the appropriate
RediSearch command:

- `FT.SEARCH` for plain search and vector KNN queries
- `FT.AGGREGATE` for `DISTINCT`, aggregate functions, `GROUP BY`, and `HAVING`
- `FT.HYBRID` for weighted vector + text queries expressed through
  `OPTION (vector_weight = ..., text_weight = ...)`

## Syntax

```text
FT.SQL "<sql-query>"
```

`FT.SQL` accepts exactly one argument: the SQL query string. The index name is
taken from the SQL `FROM` clause.

## Availability

- Status in this branch: experimental/beta
- Release TODO: item `8` remains open; `FT.SQL` is not GA/default-on yet
- Default: disabled
- Enable with:

```text
FT.CONFIG SET SQL_ENABLED true
```

- Command type: read-only
- Deployment modes: standalone and coordinator/public command path
- Current module-level coverage: standalone parity coverage for the documented
  SQL surface, plus coordinator/public-path search and aggregate parity
  coverage and coordinator vector, Hybrid, and JSON behavioral coverage

If `SQL_ENABLED` is `false`, `FT.SQL` returns an error instead of executing the
query.

## Release TODO

The SQL semantic-layer closure work is not fully complete yet. The remaining
open item is the release gate/default-on decision for `FT.SQL`.

`FT.SQL` must stay experimental/beta and `SQL_ENABLED` must stay `false` by
default until all of the following are signed off:

- no open P0/P1 SQL audit issues remain
- the supported SQL behavioral matrix is green in the deployment modes we claim
  to support
- the benchmark pairs and microbenchmarks are run on release hardware and
  reviewed
- the release evidence template in
  [tests/benchmarks/SQL_BENCHMARKS.md](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/tests/benchmarks/SQL_BENCHMARKS.md)
  is filled with release-hardware results for the target build
- rollback guidance is documented, with `SQL_ENABLED=false` as the immediate
  disable path
- a release owner explicitly approves the default-on change

## Behavior

- SQL is parsed and validated in the Rust SQL semantic layer.
- Schema-aware validation is used to distinguish supported equality on `TAG`
  fields from rejected equality on `TEXT` fields.
- Index names follow normal RediSearch conventions from the `FROM` clause.
  Practical names already used across the RediSearch test suite, such as
  `idx:all`, are accepted; the SQL validator rejects only empty names and names
  containing whitespace or control characters.
- The translated command is executed through the public RediSearch command path.
- Search and aggregate dispatch use dialect 2 internally when needed.
- Hybrid dispatch uses the live `FT.HYBRID SEARCH ... VSIM ... COMBINE LINEAR ... PARAMS`
  grammar and must not append `DIALECT`.

The response shape follows the translated backend command:

- `FT.SEARCH`-style results for plain search and vector KNN queries
- `FT.AGGREGATE`-style results for aggregates
- `FT.HYBRID`-style results for weighted Hybrid queries

## Supported Surface

| SQL surface | Backend | Notes |
|-------------|---------|-------|
| `SELECT *`, projection, aliases, `LIMIT/OFFSET` | `FT.SEARCH` | Core query surface |
| Equality on `TAG`, numeric comparisons, `BETWEEN` | `FT.SEARCH` | Schema-aware validation applies |
| `IN` / `NOT IN`, `LIKE` / `NOT LIKE`, `IS NULL` / `IS NOT NULL`, boolean composition | `FT.SEARCH` | `IS NULL` / `IS NOT NULL` require `INDEXMISSING` on the field |
| Single-column `ORDER BY` on non-aggregate queries | `FT.SEARCH` | Plain search sort |
| `DISTINCT`, aggregate functions, `GROUP BY`, `HAVING` | `FT.AGGREGATE` | Aggregate aliases are resolved into `FILTER` expressions |
| Multi-column `ORDER BY` on aggregate queries | `FT.AGGREGATE` | Plain search queries reject multi-column sort |
| pgvector-style KNN (`<->`, `<=>`, `<#>`) | `FT.SEARCH` | `LIMIT` determines `K`; default `K=10` |
| Weighted Hybrid via `OPTION (vector_weight, text_weight)` | `FT.HYBRID` | Only the live `COMBINE LINEAR` path is exposed in SQL v1 |

This supported surface is the same subset documented in the verified guide at
[docs/SQL_TEST_GUIDE.md](/Users/plambert/Documents/Work/Codex-Workplace/RediSearch/docs/SQL_TEST_GUIDE.md).

## Unsupported Surface

These forms are intentionally unsupported in SQL v1:

- Dedicated SQL full-text predicates such as `MATCH`, `CONTAINS`, or `FTS(...)`
- Equality against `TEXT` fields
- `JOIN`, subqueries, `UNION`, window functions
- Geo SQL predicates
- Hybrid-only knobs beyond `vector_weight` and `text_weight`
  Examples: `RANGE`, `RRF`, `YIELD_SCORE_AS`, scorer selection, shard tuning

## Important Limitations

- Plain search queries cannot use multiple `ORDER BY` columns.
- `OPTION (...)` is only valid when the query includes vector ordering
  (`ORDER BY field <-> '[...]'`, `<=>`, or `<#>`).
- If only one Hybrid weight is provided, the other defaults to `0.5`.
- `LIKE` / `NOT LIKE` translate to RediSearch wildcard syntax. Leading-wildcard
  patterns depend on the underlying field options, for example
  `WITHSUFFIXTRIE`.
- `FT.SQL` does not expose raw passthrough flags for `FT.SEARCH`,
  `FT.AGGREGATE`, or `FT.HYBRID`; the entire surface is expressed through SQL.

## Validation Guards

The current SQL validator rejects:

- SQL strings longer than `65536` bytes
- Condition nesting deeper than `32`
- `IN` lists larger than `1000` values
- Projections larger than `100` selected expressions
- `ORDER BY` lists larger than `8` columns

## Examples

### Plain Search

```text
FT.SQL "SELECT name, price FROM products WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
```

### Aggregation

```text
FT.SQL "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price FROM products GROUP BY category HAVING COUNT(*) >= 2 ORDER BY category ASC"
```

### Vector KNN

```text
FT.SQL "SELECT category FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.1, 0.0]' LIMIT 2"
```

### Weighted Hybrid

```text
FT.SQL "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.0, 0.0]' LIMIT 2 OPTION (vector_weight = 0.7, text_weight = 0.3)"
```

### JSON Indexes

`FT.SQL` works with JSON indexes through aliased schema fields, for example:

```text
FT.CREATE products_json ON JSON PREFIX 1 product: SCHEMA $.name AS name TEXT $.price AS price NUMERIC SORTABLE $.category AS category TAG
FT.SQL "SELECT name, price FROM products_json WHERE category = 'electronics' ORDER BY price ASC LIMIT 2"
```

## See Also

- [SQL semantic layer design](../SQL_SEMANTIC_LAYER_DESIGN.md)
- [SQL verified guide](../SQL_TEST_GUIDE.md)
