# SQL Semantic Layer Design: FT.SQL Command

## Executive Summary

**Purpose**: Enable SQL-familiar developers to query RediSearch indexes using standard SQL syntax, lowering the barrier to entry and improving developer experience.

**Approach**: Introduce an `FT.SQL` command with a Rust-based SQL parser that translates SQL queries into equivalent RediSearch command invocations.

**Performance Target**: <1ms translation overhead for cached queries; near-zero overhead for cache hits.

## Status And Source Of Truth

- [FT.SQL command reference](commands/FT.SQL.md) is the authoritative
  user-facing contract for syntax, supported surface, and limitations.
- [SQL test guide](SQL_TEST_GUIDE.md) contains only verified examples that map
  to module-level behavioral tests.
- This design document records architecture, closure targets, and historical
  design notes. If a behavior example here conflicts with the command reference
  or passing tests, follow the command reference and tests.

---

## 0. Scope Freeze (Target GA Surface)

This document mixes implemented behavior, closure targets, and future ideas. For
the SQL semantic-layer closure work, the target GA subset is the matrix below.
It overrides older aspirational examples elsewhere in this document.

The branch is still experimental/beta today and remains gated by
`SQL_ENABLED`. Freezing the scope does not imply the branch is currently GA; it
defines the exact feature set that must be made correct, tested, and documented
before the default-on decision.

| Surface | Target GA | Backend | Notes |
|---------|-----------|---------|-------|
| `SELECT *`, field projection, aliases, `LIMIT/OFFSET` | Yes | `FT.SEARCH` | Core query surface |
| Comparisons, `BETWEEN`, `IN` / `NOT IN`, `LIKE` / `NOT LIKE`, `IS NULL` / `IS NOT NULL`, simple boolean forms | Yes | `FT.SEARCH` | Schema-aware validation still applies; supported boolean forms are `a AND b`, `a OR b`, and `NOT <single predicate>` |
| Single-column `ORDER BY` on non-aggregate queries | Yes | `FT.SEARCH` | Matches current translator shape |
| Multi-column `ORDER BY` | Aggregate-only | `FT.AGGREGATE` | Plain search queries must reject it |
| `DISTINCT`, aggregate functions, `GROUP BY`, `HAVING` | Yes | `FT.AGGREGATE` | Includes the aggregate set already implemented in the Rust layer |
| pgvector-style KNN (`<->`, `<=>`, `<#>`) | Yes | `FT.SEARCH` | `LIMIT` determines `K`; default `K=10` |
| Weighted Hybrid via `OPTION(vector_weight, text_weight)` | Yes | `FT.HYBRID` | Must emit the live `SEARCH ... VSIM ... COMBINE LINEAR ... PARAMS` grammar |
| Additional Hybrid knobs (`RANGE`, `RRF`, `YIELD_SCORE_AS`, filter policy tuning, scorer selection) | No | N/A | Post-GA for SQL v1 |
| `JOIN`, subqueries, `UNION`, window functions | No | N/A | Post-GA |
| Dedicated SQL full-text predicates (`MATCH`, `FTS(...)`, `CONTAINS`) | No | N/A | Post-GA |
| Geo SQL, schema discovery SQL (`SHOW`, `DESCRIBE`) | No | N/A | Post-GA |

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              FT.SQL Command                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   SQL Query ──► SQL Parser ──► SQL AST ──► Translator ──► Command argv      │
│       │           (Rust)        (Rust)       (Rust)            │            │
│       │                                                        │            │
│       └──────────────────── Cache Layer ◄──────────────────────┘            │
│                           (sql_hash → translation)                          │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Command argv ──► Existing Query Pipeline ──► Results                      │
│               (FT.SEARCH / FT.AGGREGATE / FT.HYBRID)                        │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Language | Responsibility |
|-----------|----------|----------------|
| `sql_parser` | Rust | Parse SQL string into AST, validate syntax |
| `sql_translator` | Rust | Convert SQL AST to RediSearch command invocations |
| `sql_cache` | Rust | Cache SQL→RQL translations (hash-based) |
| `sql_parser_ffi` | Rust | C FFI bindings for integration |
| `sql_command.c` | C | Register `FT.SQL` command, orchestrate flow |

### Integration Points

1. **Command Registration**: New `FT.SQL` command in `src/module.c`
2. **FFI Boundary**: `sql_parser_ffi` crate exposes C-callable functions
3. **Query Execution**: Reuse existing `FT.SEARCH`/`FT.AGGREGATE` infrastructure
4. **Cache Integration**: Leverage existing `query_cache` infrastructure

---

## 2. SQL Subset Support

### Target GA Features

**SELECT clause**
- `SELECT *`
- `SELECT field1, field2`
- `SELECT field AS alias`
- `SELECT DISTINCT field1, field2`
- Aggregate functions exposed by RediSearch:
  - `COUNT(*)`, `COUNT(field)`
  - `SUM(field)`, `AVG(field)`, `MIN(field)`, `MAX(field)`
  - `COUNT_DISTINCT(field)`, `COUNT_DISTINCTISH(field)`, `STDDEV(field)`
  - `QUANTILE(field, percentile)`
  - `TOLIST(field)`
  - `FIRST_VALUE(field, sort_field)`
  - `FIRST_VALUE(field, sort_field, 'ASC'|'DESC')`
  - `RANDOM_SAMPLE(field, size)`
  - `HLL(field)`, `HLL_SUM(field)`

**WHERE clause**
- Comparison operators: `=`, `!=` / `<>`, `>`, `>=`, `<`, `<=`
- Ranges and sets: `BETWEEN a AND b`, `IN (...)`, `NOT IN (...)`
- Pattern matching: `LIKE`, `NOT LIKE`
- Null checks: `IS NULL`, `IS NOT NULL`
- Boolean logic: simple `a AND b`, simple `a OR b`, and `NOT <single predicate>`

**ORDER BY**
- `ORDER BY field ASC|DESC`
- `ORDER BY a ASC, b DESC` for aggregate queries translated with `FT.AGGREGATE`
- Vector distance operators using pgvector-style syntax:
  - `ORDER BY embedding <-> '[...]'` (L2)
  - `ORDER BY embedding <=> '[...]'` (Cosine)
  - `ORDER BY embedding <#> '[...]'` (Inner Product)

**GROUP BY / HAVING**
- `GROUP BY field1, field2`
- `HAVING aggregate_condition`
- `HAVING` supports aggregate comparisons plus `AND` / `OR`, and resolves aggregate aliases when building `FILTER`

**LIMIT / OFFSET**
- `LIMIT n`
- `LIMIT n OFFSET m`

**Vector Search (pgvector syntax)**
- `SELECT ... ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10`
  → `FT.SEARCH ... "*=>[KNN 10 @embedding $BLOB]" PARAMS 2 BLOB <vector>`
- `SELECT ... WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5`
  → `FT.SEARCH ... "@category:{electronics}=>[KNN 5 @embedding $BLOB]" PARAMS 2 BLOB <vector>`
- Supported distance operators: `<->` (L2), `<=>` (Cosine), `<#>` (Inner Product)
- `LIMIT` determines `K`; without `LIMIT`, `K` defaults to `10`

**Hybrid Search (SQL extension via `OPTION`)**
- `SELECT ... WHERE category = 'electronics' ORDER BY embedding <-> '[...]' LIMIT 10 OPTION (vector_weight = 0.7, text_weight = 0.3)`
  → `FT.HYBRID`
- Weighted scoring between vector and structured-filter results is configurable through `vector_weight` and `text_weight`
- If only one weight is provided, the other defaults to `0.5`
- The SQL v1 target is the live Hybrid grammar:
  `FT.HYBRID <index> SEARCH <query> VSIM @field $BLOB KNN 2 K <k> COMBINE LINEAR ... PARAMS 2 BLOB <blob>`
- SQL v1 does not expose Hybrid-only knobs beyond `vector_weight` and `text_weight`

**Translation Cache**
- Thread-safe LRU cache for SQL → RQL translations
- Cache keys combine raw SQL text with the schema version
- Cache statistics include hits, misses, and hit rate

### Not Yet Supported

- `JOIN` / multi-index queries
- Subqueries
- `UNION`
- Window functions
- Complex nested boolean `WHERE` expressions such as `(a AND b) OR c`,
  `a OR (b AND c)`, or `NOT (a AND b)`
- Dedicated full-text predicate syntax such as `MATCH`, `FTS(...)`, or `CONTAINS`
- Geo queries
- Multi-column `ORDER BY` for non-aggregate queries
- SQL exposure of advanced `FT.HYBRID` clauses such as `RANGE`, `RRF`, `YIELD_SCORE_AS`, scorer selection, and vector-side `FILTER`

---

## 3. Translation Rules

### SELECT Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `SELECT *` | No RETURN clause | Returns all indexed fields |
| `SELECT a, b` | `RETURN 2 a b` | Explicit field selection |
| `SELECT a AS alias` | `RETURN 3 a AS alias` | Alias is emitted in the RETURN clause |
| `SELECT DISTINCT a, b` | `FT.AGGREGATE ... GROUPBY 2 @a @b` | `DISTINCT` switches to aggregate mode |
| `SELECT COUNT(*)` | `FT.AGGREGATE ... REDUCE COUNT 0 AS count` | Aggregate without `GROUP BY` uses `GROUPBY 0` |
| `SELECT COUNT(field)` | `FT.AGGREGATE ... REDUCE COUNT 1 @field AS count_field` | Counts non-null field values |
| `SELECT SUM(field)` | `FT.AGGREGATE ... REDUCE SUM 1 @field AS sum_field` | |
| `SELECT AVG(field)` | `FT.AGGREGATE ... REDUCE AVG 1 @field AS avg_field` | |
| `SELECT MIN(field)` | `FT.AGGREGATE ... REDUCE MIN 1 @field AS min_field` | |
| `SELECT MAX(field)` | `FT.AGGREGATE ... REDUCE MAX 1 @field AS max_field` | |
| `SELECT COUNT_DISTINCT(field)` | `FT.AGGREGATE ... REDUCE COUNT_DISTINCT 1 @field AS count_distinct_field` | Exact cardinality |
| `SELECT COUNT_DISTINCTISH(field)` | `FT.AGGREGATE ... REDUCE COUNT_DISTINCTISH 1 @field AS count_distinctish_field` | Approximate cardinality |
| `SELECT STDDEV(field)` | `FT.AGGREGATE ... REDUCE STDDEV 1 @field AS stddev_field` | |
| `SELECT QUANTILE(field, 0.99)` | `FT.AGGREGATE ... REDUCE QUANTILE 2 @field 0.99 AS quantile_99_field` | Percentile must be between `0.0` and `1.0` |
| `SELECT TOLIST(field)` | `FT.AGGREGATE ... REDUCE TOLIST 1 @field AS tolist_field` | Collects values |
| `SELECT FIRST_VALUE(field, sort_field, 'DESC')` | `FT.AGGREGATE ... REDUCE FIRST_VALUE 4 @field BY @sort_field DESC AS first_value_field` | SQL surface uses simplified argument syntax |
| `SELECT RANDOM_SAMPLE(field, 5)` | `FT.AGGREGATE ... REDUCE RANDOM_SAMPLE 2 @field 5 AS random_sample_5_field` | Sample size must be between `1` and `1000` |
| `SELECT HLL(field)` | `FT.AGGREGATE ... REDUCE HLL 1 @field AS hll_field` | Raw HyperLogLog value |
| `SELECT HLL_SUM(field)` | `FT.AGGREGATE ... REDUCE HLL_SUM 1 @field AS hll_sum_field` | Merge HyperLogLog values |

### FROM Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `FROM idx` | Index name parameter | Required, must exist |
| `FROM idx AS alias` | Alias tracked internally | Multi-index queries are still unsupported |

### WHERE Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `field = 'value'` | `@field:{value}` | Exact TAG-style match |
| `field = 42` | `@field:[42 42]` | Numeric equality becomes an exact range |
| `field > 100` | `@field:[(100 +inf]` | Exclusive lower bound |
| `field >= 100` | `@field:[100 +inf]` | Inclusive lower bound |
| `field < 100` | `@field:[-inf (100]` | Exclusive upper bound |
| `field <= 100` | `@field:[-inf 100]` | Inclusive upper bound |
| `field BETWEEN a AND b` | `@field:[a b]` | Inclusive range |
| `field != 'value'` | `-@field:{value}` | Negated TAG match |
| `field <> 'value'` | `-@field:{value}` | Same as `!=` |
| `field != 42` | `-@field:[42 42]` | Negated numeric equality |
| `field IN ('a','b','c')` | `@field:{a|b|c}` | String-set membership |
| `field NOT IN ('a','b')` | `-@field:{a|b}` | Negated TAG set |
| `field IN (10,20,30)` | `(@field:[10 10]|@field:[20 20]|@field:[30 30])` | Numeric `IN` expands to OR of exact ranges |
| `field NOT IN (10,20)` | `-(@field:[10 10]|@field:[20 20])` | Negated numeric OR |
| `field LIKE '%word%'` | `@field:*word*` | Contains pattern |
| `field LIKE 'word%'` | `@field:word*` | Prefix pattern |
| `field LIKE '%word'` | `@field:*word` | Suffix pattern |
| `field NOT LIKE '%word%'` | `-@field:*word*` | Negated pattern |
| `a AND b` | `(@a) (@b)` | Implicit intersection |
| `a OR b` | `(@a) \| (@b)` | Union operator |
| `NOT condition` | `-(@condition)` | Negation prefix |
| `field IS NULL` | `ismissing(@field)` | Field missing |
| `field IS NOT NULL` | `-ismissing(@field)` | Field present |

### ORDER BY Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `ORDER BY field` | `SORTBY field ASC` | Default direction is ascending |
| `ORDER BY field ASC` | `SORTBY field ASC` | Explicit ascending |
| `ORDER BY field DESC` | `SORTBY field DESC` | Descending order |
| `ORDER BY a ASC, b DESC` | `FT.AGGREGATE ... SORTBY 4 @a ASC @b DESC` | Multi-column sort is supported for aggregate queries |
| `ORDER BY embedding <-> '[...]'` | `FT.SEARCH ... "*=>[KNN k @embedding $BLOB]" PARAMS 2 BLOB <vector>` | L2 / Euclidean vector search |
| `ORDER BY embedding <=> '[...]'` | `FT.SEARCH ... "*=>[KNN k @embedding $BLOB]" PARAMS 2 BLOB <vector>` | Cosine vector search |
| `ORDER BY embedding <#> '[...]'` | `FT.SEARCH ... "*=>[KNN k @embedding $BLOB]" PARAMS 2 BLOB <vector>` | Inner-product vector search |

### LIMIT/OFFSET Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `LIMIT n` | `LIMIT 0 n` | First n results |
| `LIMIT n OFFSET m` | `LIMIT m n` | Skip m, take n |
| `OFFSET m LIMIT n` | `LIMIT m n` | Same as above |

### GROUP BY Clause (Triggers FT.AGGREGATE)

| SQL | RQL Translation |
|-----|-----------------|
| `GROUP BY field` | `GROUPBY 1 @field` |
| `GROUP BY a, b` | `GROUPBY 2 @a @b` |

### HAVING Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `HAVING COUNT(*) > 5` | `FILTER @count>5` | Applied after `GROUPBY` |
| `SELECT category, COUNT(*) AS cnt ... HAVING COUNT(*) >= 5` | `FILTER @cnt>=5` | Aggregate aliases are resolved automatically |
| `HAVING COUNT(*) > 10 OR SUM(price) > 1000` | `FILTER (@count>10 || @sum_price>1000)` | Boolean logic uses `&&` / `||` in `FILTER` |

### Vector Search / KNN

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `SELECT * FROM idx ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10` | `FT.SEARCH idx "*=>[KNN 10 @embedding $BLOB]" PARAMS 2 BLOB [0.1, 0.2]` | Pure KNN query |
| `SELECT * FROM idx WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5` | `FT.SEARCH idx "@category:{electronics}=>[KNN 5 @embedding $BLOB]" PARAMS 2 BLOB [0.1, 0.2]` | Filter + KNN |
| `SELECT * FROM idx ORDER BY embedding <#> '[0.1, 0.2]'` | `FT.SEARCH idx "*=>[KNN 10 @embedding $BLOB]" PARAMS 2 BLOB [0.1, 0.2]` | `LIMIT` omitted → `K = 10` |

### Hybrid Search / FT.HYBRID

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `SELECT * FROM idx ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10 OPTION (vector_weight = 0.7, text_weight = 0.3)` | `FT.HYBRID idx SEARCH "*" VSIM @embedding $BLOB KNN 2 K 10 COMBINE LINEAR 4 ALPHA 0.7 BETA 0.3 PARAMS 2 BLOB [0.1, 0.2]` | `OPTION` switches the command to `FT.HYBRID` and must use the live parser grammar |
| `SELECT * FROM idx WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5 OPTION (vector_weight = 0.6, text_weight = 0.4)` | `FT.HYBRID idx SEARCH "@category:{electronics}" VSIM @embedding $BLOB KNN 2 K 5 COMBINE LINEAR 4 ALPHA 0.6 BETA 0.4 PARAMS 2 BLOB [0.1, 0.2]` | Structured filters are preserved in the Hybrid search subquery |
| `OPTION (vector_weight = 0.6)` | `vector_weight = 0.6`, `text_weight = 0.5` | Unspecified weight defaults to `0.5` |

---

## 4. Implementation Plan

### Rust Crates Structure

```
src/redisearch_rs/
├── sql_parser/                    # Core SQL parsing and translation
│   ├── src/
│   │   ├── lib.rs                 # Public API
│   │   ├── parser/                # SQL parsing modules
│   │   │   ├── mod.rs             # SQL parsing wrapper
│   │   │   ├── preprocessor.rs    # OPTION clause and FROM identifier preprocessing
│   │   │   ├── select.rs          # SELECT statement parsing
│   │   │   ├── aggregates.rs      # Aggregate expression parsing
│   │   │   ├── expressions.rs     # WHERE / ORDER BY expression parsing
│   │   │   └── options.rs         # Hybrid OPTION clause application
│   │   ├── ast.rs                 # Internal AST representation
│   │   ├── translator.rs          # SQL AST → RQL conversion
│   │   ├── validation.rs          # Schema validation, type checking
│   │   └── cache.rs               # SQL→RQL translation cache
│   └── Cargo.toml
│
├── c_entrypoint/
│   └── sql_parser_ffi/            # C FFI bindings
│       ├── src/lib.rs             # #[no_mangle] extern "C" functions
│       └── Cargo.toml
│
└── headers/
    └── sql_parser_ffi.h           # Auto-generated C header
```

### External Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `sqlparser` | `0.54.0` | SQL parsing and AST generation |

**Why `sqlparser`?**
- Widely used, including by DataFusion and Apache Arrow
- Supports multiple SQL dialects; the current parser uses `PostgreSqlDialect`
  so pgvector operators like `<->`, `<=>`, and `<#>` are parsed correctly
- Produces well-structured AST
- Active maintenance, good documentation

### C Integration

**Current runtime flow** (`src/sql_command.c`)

1. Parse `FT.SQL <sql_query>`.
2. Probe the query once to discover the target index and collect schema
   metadata for validation/cache invalidation.
3. Call `sql_translate_cached_with_schema(...)` with the current schema
   revision and field capabilities.
4. Dispatch based on the translated command enum:
   - `Search` → `FT.SEARCH`
   - `Aggregate` → `FT.AGGREGATE`
   - `Hybrid` → `FT.HYBRID`
5. Append `DIALECT 2` only for `FT.SEARCH` / `FT.AGGREGATE`. Hybrid dispatch
   must use the live `FT.HYBRID SEARCH ... VSIM ... COMBINE LINEAR ... PARAMS`
   grammar and must not append `DIALECT`.
6. Use the public command path in both standalone and coordinator deployments;
   `FT.SQL` is registered with `noKeyArgs` because the index is embedded in the
   SQL string.

> Known limitation: the runtime still does the probe translation through the
> uncached `sql_translate(...)` path before it can call the schema-aware cached
> path. Avoiding that extra translation on cache hits is a future optimization
> opportunity.

### Key Interfaces

**Rust Public API** (`sql_parser/src/lib.rs`):

```rust
/// Translate SQL query to RQL command
pub fn translate(sql: &str) -> Result<Translation, SqlError>;

/// Translation result
pub struct Translation {
    pub command: Command,         // Search, Aggregate, or Hybrid
    pub index_name: String,
    pub query_string: String,
    pub arguments: Vec<String>,   // Additional RQL arguments
}

pub enum Command {
    Search,     // FT.SEARCH
    Aggregate,  // FT.AGGREGATE
    Hybrid,     // FT.HYBRID
}
```

**FFI Layer** (`sql_parser_ffi/src/lib.rs`):

```rust
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate(
    sql: *const u8,
    sql_len: usize,
) -> SqlTranslationResult;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached(
    sql: *const u8,
    sql_len: usize,
) -> SqlTranslationResult;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translate_cached_with_schema(
    sql: *const u8,
    sql_len: usize,
    schema_version: u64,
    fields: *const SqlSchemaField,
    fields_len: usize,
) -> SqlTranslationResult;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sql_translation_result_free(result: SqlTranslationResult);
```

All three translation entry points route through shared helpers that wrap the
call in `catch_unwind(...)` and validate the incoming SQL bytes before
translation. The FFI boundary rejects null pointers, embedded NUL bytes, and
invalid UTF-8; the schema-aware entry point also validates the schema field
array and field names.

---

## 5. Error Handling

### Error Categories

| Category | Code | Example |
|----------|------|---------|
| Syntax Error | `SQL_SYNTAX_ERROR` | Invalid SQL syntax |
| Unsupported Feature | `SQL_UNSUPPORTED` | Feature not yet implemented |
| Schema Error | `SQL_SCHEMA_ERROR` | Index/field doesn't exist |
| Type Error | `SQL_TYPE_ERROR` | Incompatible operation on field type |
| Translation Error | `SQL_TRANSLATION_ERROR` | Cannot express in RQL |

### Error Message Format

```
ERR SQL Error: <category>: <message> at position <pos>
```

**Examples**:
```
ERR SQL Error: Syntax error: Expected 'FROM' but found 'FORM' at position 12
ERR SQL Error: Unsupported feature: FULL OUTER JOIN is not supported at position 45
ERR SQL Error: Schema error: Field 'nonexistent' not found in index 'products'
ERR SQL Error: Type error: Cannot compare TEXT field 'title' with numeric value at position 28
```

### Error Struct (Rust)

```rust
pub struct SqlError {
    pub category: ErrorCategory,
    pub message: String,
    pub position: Option<usize>,   // Character position in SQL string
    pub suggestion: Option<String>, // "Did you mean...?"
}
```

### Graceful Degradation

For partially supported features, provide helpful errors:

```sql
-- User query
SELECT * FROM idx WHERE MATCH(title) AGAINST('search terms')

-- Error response
ERR SQL Error: Unsupported feature: MATCH...AGAINST syntax is not supported.
Use: SELECT * FROM idx WHERE title = 'search terms' (for exact match)
  or FT.SEARCH idx '@title:search terms' (for full-text search)
```

---

## 6. Performance Considerations

### SQL→RQL Translation Cache

**Strategy**: Hash the incoming SQL string together with the schema version,
cache the translated RQL result, and evict least-recently-used entries when the
cache reaches capacity.

```rust
pub struct TranslationCache {
    cache: HashMap<u64, Translation>,
    lru_queue: VecDeque<u64>,
    config: CacheConfig,
}
```

**Cache Key**:
- Uses Rust's `DefaultHasher`
- Hash input is the raw SQL string exactly as received plus the schema version
- Query text is not rewritten before hashing, so formatting-only differences do
  **not** share cache entries

**Translation Cache Defaults (internal)**:
- Default capacity: 1000 queries
- Eviction: LRU when at capacity

> Note: The Rust SQL translation cache is an internal implementation detail and
> is not exposed as an operator-facing configuration.

### Integration with Query Cache

The SQL layer's translation cache is **separate** from the query result cache (`query_cache`):

```
SQL Query ──► [Translation Cache] ──► Translation ──► [Query Cache] ──► Results
              (SQL→command mapping)                  (backend-query→DocIds mapping)
```

Both caches work together:
1. **Translation Cache**: Avoids re-parsing SQL
2. **Query Cache**: Avoids re-executing query

### Performance Targets

| Operation | Target Latency | Notes |
|-----------|----------------|-------|
| SQL Parsing | <500µs | Cold parse, no cache |
| AST Translation | <200µs | Simple queries |
| Translation Cache Hit | <10µs | Hash lookup only |
| End-to-end (cached) | <1ms | Total SQL→RQL overhead |

### Benchmarks Required

1. **Translation throughput**: Queries/second for parse+translate
2. **Cache hit rate**: In realistic workloads
3. **Memory overhead**: Per cached translation
4. **Comparison**: `FT.SQL` vs equivalent `FT.SEARCH` latency

### Benchmark Artifacts

- Rust microbenchmarks:
  `src/redisearch_rs/sql_parser/benches/sql_translation.rs`
- End-to-end benchmark runbook:
  `tests/benchmarks/SQL_BENCHMARKS.md`
- Search SQL benchmark pair:
  `tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-fulltext-search-sortby-limit-0-100.yml`
  and
  `tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-sql-search-sortby-limit-0-100.yml`
- Aggregate SQL benchmark pair:
  `tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-groupby-title-limit-0-100.yml`
  and
  `tests/benchmarks/search-ftsb-10K-enwiki_abstract-hashes-sql-groupby-title-limit-0-100.yml`
- Vector SQL benchmark pair:
  `tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql-surface.yml`
  and
  `tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-tag-filter-sql.yml`
- Hybrid SQL benchmark pair:
  `tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql-surface.yml`
  and
  `tests/benchmarks/vecsim-arxiv-titles-384-angular-filters-m16-ef-128-hybrid-tag-filter-sql.yml`

### Sign-Off Expectations

- run the Criterion microbench before release review and capture uncached,
  cached, and schema-churn numbers
- run each SQL YAML against its native baseline on the same setup and compare
  p50/p95 latency plus throughput
- treat the numbers as release evidence, not as static golden values committed
  into the repo

---

## 7. Security Considerations

### SQL Injection

**Not applicable**: We translate SQL to RQL, we don't execute raw SQL. The RQL query is constructed programmatically from the parsed AST, not by string concatenation.

```rust
// SAFE: AST-based construction
fn translate_equality(field: &str, value: &str) -> String {
    // Value is properly escaped by RQL escaping rules
    format!("@{}:{}", escape_field(field), escape_value(value))
}

// We NEVER do string interpolation of raw input
```

### Input Validation

1. **Query size limit**: Maximum 64KB SQL query
2. **Nesting depth limit**: Maximum 32 levels of nested expressions
3. **IN clause limit**: Maximum 1000 values in single IN clause
4. **Structural limits instead of timeout**: GA v1 relies on deterministic
   complexity limits rather than an in-process translation timeout

### Query Complexity Limits

To prevent denial-of-service via complex queries:

```rust
const MAX_QUERY_SIZE: usize = 65536;        // 64KB
const MAX_NESTING_DEPTH: usize = 32;
const MAX_IN_VALUES: usize = 1000;
const MAX_SELECT_COLUMNS: usize = 100;
const MAX_ORDER_BY_COLUMNS: usize = 8;
```

---

## 8. Testing Strategy

### Unit Tests (Rust)

Location: `src/redisearch_rs/sql_parser/src/parser/*_tests.rs`

**Coverage Areas**:
1. Parser tests for each SQL construct
2. Translator tests for each translation rule
3. Error handling for invalid inputs
4. Edge cases (empty strings, special characters, Unicode)

```rust
#[test]
fn test_simple_select_star() {
    let result = translate("SELECT * FROM products").unwrap();
    assert_eq!(result.index_name, "products");
    assert_eq!(result.query_string, "*");
    assert!(result.arguments.is_empty());
}

#[test]
fn test_where_equality() {
    let result = translate("SELECT * FROM idx WHERE status = 'active'").unwrap();
    assert_eq!(result.query_string, "@status:{active}");
}

#[test]
fn test_between_translation() {
    let result = translate("SELECT * FROM idx WHERE price BETWEEN 10 AND 100").unwrap();
    assert_eq!(result.query_string, "@price:[10 100]");
}
```

### Integration Tests (Rust)

Location: `src/redisearch_rs/sql_parser/tests/integration.rs`

**Test Categories**:
1. Basic query comparison: `FT.SQL` vs equivalent `FT.SEARCH`
2. Aggregation comparison: `FT.SQL` vs equivalent `FT.AGGREGATE`
3. Error cases: Syntax errors, unsupported features
4. Edge cases: Special characters, empty results, large result sets

```python
def test_sql_select_star(env):
    """FT.SQL 'SELECT * FROM idx' should match FT.SEARCH idx '*'"""
    # Setup index with test data
    conn = getConnectionByEnv(env)

    # Run SQL query
    sql_result = conn.execute_command('FT.SQL', "SELECT * FROM idx")

    # Run equivalent RQL query
    rql_result = conn.execute_command('FT.SEARCH', 'idx', '*')

    # Compare results
    assert_results_equal(sql_result, rql_result)

def test_sql_syntax_error(env):
    """Invalid SQL should return helpful error"""
    conn = getConnectionByEnv(env)

    with pytest.raises(ResponseError) as exc:
        conn.execute_command('FT.SQL', "SELEC * FROM idx")  # Typo

    assert "SQL Error: Syntax error" in str(exc.value)
```

### Performance Tests

Locations:
- `src/redisearch_rs/sql_parser/benches/sql_translation.rs`
- `tests/benchmarks/SQL_BENCHMARKS.md`

The performance test plan is intentionally split:
1. Criterion microbenchmarks measure translation overhead, cache hits, cache
   misses, and cache stats overhead inside the Rust SQL layer.
2. `redisbench-admin` YAML benchmarks compare `FT.SQL` to equivalent native
   `FT.SEARCH`, `FT.AGGREGATE`, and `FT.HYBRID` command shapes on the same
   datasets.

The SQL release review should use those artifacts to capture real measurements
on release hardware rather than rely on a placeholder script or hard-coded
assertion.

`tests/benchmarks/SQL_BENCHMARKS.md` includes the in-repo workflow and release
evidence template. The actual release-hardware measurements are intentionally an
external sign-off artifact and are not closed by this branch alone.

---

## 9. Migration Path

### Feature Flag

```c
// Configuration option
#define SQL_ENABLED_DEFAULT false

// Runtime toggle
FT.CONFIG SET SQL_ENABLED true|false
```

### Documentation Updates

1. **Command reference**: `commands/FT.SQL.md` - authoritative supported surface
2. **Verified guide**: `SQL_TEST_GUIDE.md` - examples aligned with passing behavioral tests
3. **Command metadata**: `commands.json` - machine-readable `FT.SQL` entry for command docs/tooling

### Rollout Phases

> Status note (2026-03-30): the earlier phase-based feature roadmap in this
> document is historical. The current source of truth for feature surface is the
> supported subset documented in Sections 2 and 3. The rollout bullets below are
> about exposure and operational posture, not about withholding already-
> implemented SQL features.

**Alpha** (internal testing):
- Feature flag off by default
- Limited to the currently audited supported subset
- Extensive logging for debugging

**Beta** (opt-in users):
- Feature flag configurable
- Hardened supported subset documented above
- Performance monitoring

**GA** (general availability):
- Feature flag on by default
- Requires separate validation and rollout sign-off
- Additional SQL surface area only after separate validation
- The target GA surface is the scope-freeze matrix above; anything outside that matrix stays post-GA even if discussed elsewhere in this design doc

### Remaining TODO: Item 8 (Release Gate)

Status: open.

This is the remaining closure item after the implementation, hardening,
coverage, documentation, and benchmark work. Until this item is closed,
`FT.SQL` remains experimental/beta and `SQL_ENABLED` stays `false` by default.

The release gate is:

- keep `SQL_ENABLED=false` by default in the shipped configuration
- require no open P0/P1 SQL semantic-layer audit issues
- require the supported SQL behavioral matrix to pass in every deployment mode
  claimed by the command reference
- require benchmark evidence from the SQL microbenchmarks and the paired native
  versus SQL end-to-end benchmarks
- require the release evidence template from `tests/benchmarks/SQL_BENCHMARKS.md`
  to be filled with release-hardware results for the target build
- require an explicit rollback plan, with `FT.CONFIG SET SQL_ENABLED false` as
  the immediate disable path
- require an explicit release-owner sign-off before any default-on change

Closing item `8` means the branch can make a deliberate GA/default-on decision.
Until then, the branch may be feature-complete for the audited SQL surface, but
it is not yet release-complete.

---

## 10. Open Questions / Future Work

### Resolved: Full-Text Search Surface

**Decision**: the current SQL surface does **not** introduce a dedicated
full-text predicate such as `MATCH`, `FTS(...)`, or `CONTAINS`.

- Exact matching remains `field = 'value'` / `IN (...)` on TAG-capable fields
- Dedicated full-text predicate syntax remains outside the supported subset
- This keeps the implemented surface aligned with the current parser and
  translator behavior

### Resolved: Vector Search Syntax

**Decision**: vector search uses pgvector-style operators in `ORDER BY`.

```sql
SELECT * FROM idx ORDER BY embedding <-> '[0.1, 0.2, ...]' LIMIT 10
SELECT * FROM idx ORDER BY embedding <=> '[0.1, 0.2, ...]' LIMIT 10
SELECT * FROM idx ORDER BY embedding <#> '[0.1, 0.2, ...]' LIMIT 10
```

- `<->` = L2 distance
- `<=>` = Cosine distance
- `<#>` = Inner Product
- Translation target is RediSearch KNN syntax via `FT.SEARCH ... =>[KNN ...]`
- The query operator records user intent; the index configuration still decides the effective distance metric at execution time

### Resolved: Hybrid Search Syntax

**Decision**: weighted hybrid vector search uses the SQL `OPTION` clause.

```sql
SELECT * FROM idx
WHERE category = 'electronics'
ORDER BY embedding <-> '[0.1, 0.2, ...]'
LIMIT 10
OPTION (vector_weight = 0.7, text_weight = 0.3)
```

- `OPTION` requires a vector `ORDER BY`
- The translator target is the live `FT.HYBRID SEARCH ... VSIM ... COMBINE LINEAR ... PARAMS` grammar
- `vector_weight` and `text_weight` default to `0.5` when omitted
- SQL v1 does not expose other Hybrid-only clauses

### Geospatial Queries

**Question**: How to express geo queries in SQL?

```sql
-- Radius search
SELECT * FROM idx WHERE DISTANCE(location, POINT(-122.4, 37.8)) < 10 km

-- Bounding box
SELECT * FROM idx WHERE location WITHIN BOX(...)
```

**Recommendation**: still future work; no SQL geo surface is implemented today.

### Schema Discovery

**Question**: Should we support SHOW/DESCRIBE commands?

```sql
SHOW INDEXES;
DESCRIBE idx;
SHOW COLUMNS FROM idx;
```

**Recommendation**: still future work; likely maps to `FT._LIST` and `FT.INFO` if exposed later.

---

## Appendix A: Complete Translation Examples

### Example 1: E-commerce Product Search

```sql
-- SQL
SELECT name, price, category
FROM products
WHERE category = 'electronics'
  AND price BETWEEN 100 AND 500
ORDER BY price ASC
LIMIT 20

-- Translates to
FT.SEARCH products "@category:{electronics} @price:[100 500]"
    RETURN 3 name price category
    SORTBY price ASC
    LIMIT 0 20
```

### Example 2: User Analytics Aggregation

```sql
-- SQL
SELECT country, COUNT(*), AVG(age)
FROM users
WHERE status = 'active'
GROUP BY country

-- Translates to
FT.AGGREGATE users "@status:{active}"
    GROUPBY 1 @country
    REDUCE COUNT 0 AS count
    REDUCE AVG 1 @age AS avg_age
```

### Example 3: Complex Boolean Logic

```sql
-- SQL
SELECT * FROM articles
WHERE (category = 'tech' OR category = 'science')
  AND NOT (status = 'draft')
  AND published_date >= '2024-01-01'

-- Translates to
FT.SEARCH articles
    "((@category:{tech}) | (@category:{science})) -(@status:{draft}) @published_date:[2024-01-01 +inf]"
```

---

## Appendix B: Unsupported SQL Features

| Feature | Reason | Alternative |
|---------|--------|-------------|
| `JOIN` | Multiple index queries are not supported | Use application-level joins |
| `UNION` | No equivalent in RQL | Run multiple queries |
| `CASE WHEN` | No conditional expressions in RQL | Use application logic |
| `MATCH` / `FTS(...)` / `CONTAINS` | No dedicated full-text predicate syntax in the current SQL surface | Use exact TAG-style matching where applicable |
| `Subqueries` | Query composition not supported | Flatten to single query |
| `Window Functions` | No OVER/PARTITION BY | Use FT.AGGREGATE |
| `Geo queries` | No SQL geo predicate syntax is implemented | Use RediSearch geo commands directly |
| `OUTER JOIN` | Only intersection semantics | Not planned |

---

## Appendix C: Current Operator-Facing Configuration

| Config Key | Type | Default | Description |
|------------|------|---------|-------------|
| `SQL_ENABLED` | bool | false | Enable the experimental `FT.SQL` command |

> Note: The Rust SQL translation cache is an internal implementation detail and
> is not exposed as an operator-facing configuration.
