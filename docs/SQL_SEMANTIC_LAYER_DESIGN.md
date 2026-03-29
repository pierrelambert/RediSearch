# SQL Semantic Layer Design: FT.SQL Command

## Executive Summary

**Purpose**: Enable SQL-familiar developers to query RediSearch indexes using standard SQL syntax, lowering the barrier to entry and improving developer experience.

**Approach**: Introduce an `FT.SQL` command with a Rust-based SQL parser that translates SQL queries into equivalent RQL (RediSearch Query Language) commands.

**Performance Target**: <1ms translation overhead for cached queries; near-zero overhead for cache hits.

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              FT.SQL Command                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   SQL Query ──► SQL Parser ──► SQL AST ──► Translator ──► RQL String       │
│       │           (Rust)        (Rust)       (Rust)            │            │
│       │                                                        │            │
│       └──────────────────── Cache Layer ◄──────────────────────┘            │
│                              (sql_hash → rql_string)                        │
│                                                                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   RQL String ──► Existing Query Pipeline ──► Results                       │
│                    (FT.SEARCH/FT.AGGREGATE)                                 │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Language | Responsibility |
|-----------|----------|----------------|
| `sql_parser` | Rust | Parse SQL string into AST, validate syntax |
| `sql_translator` | Rust | Convert SQL AST to RQL commands |
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

### Supported Features

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
- Boolean logic: `AND`, `OR`, `NOT`

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

**Translation Cache**
- Thread-safe LRU cache for SQL → RQL translations
- Cache keys are schema-aware and include field-shape metadata
- Cache statistics include hits, misses, and hit rate

### Not Yet Supported

- `JOIN` / multi-index queries
- Subqueries
- `UNION`
- Window functions
- Dedicated full-text predicate syntax such as `MATCH`, `FTS(...)`, or `CONTAINS`
- Geo queries

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
| `SELECT * FROM idx ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10 OPTION (vector_weight = 0.7, text_weight = 0.3)` | `FT.HYBRID idx "*" VECTOR embedding K 10 VECTOR_BLOB [0.1, 0.2] WEIGHT 0.7 TEXT 0.3 LIMIT 0 10` | `OPTION` switches the command to `FT.HYBRID` |
| `SELECT * FROM idx WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5 OPTION (vector_weight = 0.6, text_weight = 0.4)` | `FT.HYBRID idx "@category:{electronics}" VECTOR embedding K 5 VECTOR_BLOB [0.1, 0.2] WEIGHT 0.6 TEXT 0.4 LIMIT 0 5` | Structured filters are preserved in the hybrid query string |
| `OPTION (vector_weight = 0.6)` | `vector_weight = 0.6`, `text_weight = 0.5` | Unspecified weight defaults to `0.5` |

---

## 4. Implementation Plan

### Rust Crates Structure

```
src/redisearch_rs/
├── sql_parser/                    # Core SQL parsing and translation
│   ├── src/
│   │   ├── lib.rs                 # Public API
│   │   ├── parser.rs              # SQL parsing wrapper
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
| `sqlparser` | 0.52+ | SQL dialect parsing (ANSI SQL compliant) |

**Why `sqlparser`?**
- Battle-tested, used by DataFusion, Apache Arrow, etc.
- Supports multiple SQL dialects (we'll use Generic/ANSI)
- Produces well-structured AST
- Active maintenance, good documentation

### C Integration

**New File**: `src/sql_command.c`

```c
// Command registration
int FT_SQLCommand(RedisModuleCtx *ctx, RedisModuleString **argv, int argc);

// FFI declarations (from sql_parser_ffi.h)
typedef struct SqlTranslationResult {
    char *rql_query;      // NULL on error
    char *error_message;  // NULL on success
    int is_aggregate;     // 1 if should use FT.AGGREGATE
} SqlTranslationResult;

SqlTranslationResult sql_translate(const char *sql, const char *index_name);
void sql_translation_result_free(SqlTranslationResult result);
```

**Command Flow**:
```c
int FT_SQLCommand(RedisModuleCtx *ctx, RedisModuleString **argv, int argc) {
    // 1. Parse arguments: FT.SQL <sql_query>
    // 2. Call Rust: sql_translate(sql, NULL)
    // 3. If error, return error message
    // 4. If is_aggregate, dispatch to FT.AGGREGATE
    // 5. Otherwise, dispatch to FT.SEARCH
    // 6. Free result
}
```

### Key Interfaces

**Rust Public API** (`sql_parser/src/lib.rs`):

```rust
/// Translate SQL query to RQL command
pub fn translate(sql: &str) -> Result<Translation, SqlError>;

/// Translation result
pub struct Translation {
    pub command: Command,         // Search or Aggregate
    pub index_name: String,
    pub query_string: String,
    pub arguments: Vec<String>,   // Additional RQL arguments
}

pub enum Command {
    Search,     // FT.SEARCH
    Aggregate,  // FT.AGGREGATE
}
```

**FFI Layer** (`sql_parser_ffi/src/lib.rs`):

```rust
#[no_mangle]
pub extern "C" fn sql_translate(
    sql: *const c_char,
    index_name: *const c_char,
) -> SqlTranslationResult;

#[no_mangle]
pub extern "C" fn sql_translation_result_free(result: SqlTranslationResult);
```

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

**Strategy**: Hash SQL string, cache translated RQL result.

```rust
pub struct TranslationCache {
    cache: HashMap<u64, CachedTranslation>,  // SQL hash → translation
    max_entries: usize,
    ttl_seconds: u64,
}

struct CachedTranslation {
    translation: Translation,
    created_at: Instant,
    hit_count: u64,
}
```

**Cache Key**: FNV-1a hash of normalized SQL string (whitespace-normalized, case-normalized).

**Translation Cache Defaults (internal)**:
- Default capacity: 1000 queries
- Default TTL: 300 seconds (5 minutes)
- Eviction: LRU when at capacity

> Note: The Rust SQL translation cache is an internal implementation detail and
> is not exposed as an operator-facing configuration.

### Integration with Query Cache

The SQL layer's translation cache is **separate** from the query result cache (`query_cache`):

```
SQL Query ──► [Translation Cache] ──► RQL String ──► [Query Cache] ──► Results
              (SQL→RQL mapping)                      (RQL→DocIds mapping)
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
4. **Timeout**: Translation timeout of 100ms (prevents pathological queries)

### Query Complexity Limits

To prevent denial-of-service via complex queries:

```rust
const MAX_QUERY_SIZE: usize = 65536;        // 64KB
const MAX_NESTING_DEPTH: usize = 32;
const MAX_IN_VALUES: usize = 1000;
const MAX_SELECT_COLUMNS: usize = 100;
const MAX_ORDER_BY_COLUMNS: usize = 8;
const TRANSLATION_TIMEOUT_MS: u64 = 100;
```

---

## 8. Testing Strategy

### Unit Tests (Rust)

Location: `src/redisearch_rs/sql_parser/src/tests/`

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

### Integration Tests (Python)

Location: `tests/pytests/test_sql.py`

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

Location: `tests/benchmark/sql_benchmark.py`

```python
def benchmark_translation_overhead():
    """Measure SQL→RQL translation overhead"""
    queries = load_test_queries()

    for query in queries:
        sql_time = timeit(lambda: conn.execute_command('FT.SQL', query['sql']))
        rql_time = timeit(lambda: conn.execute_command('FT.SEARCH', *query['rql']))

        overhead = (sql_time - rql_time) / rql_time * 100
        assert overhead < 10, f"Translation overhead {overhead}% exceeds 10%"
```

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

1. **New page**: `docs/commands/FT.SQL.md` - Command reference
2. **Examples page**: Common SQL queries and their RQL equivalents
3. **Migration guide**: For users coming from SQL databases
4. **Limitations page**: What's not supported and why

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
- The translator emits `FT.HYBRID`
- `vector_weight` and `text_weight` default to `0.5` when omitted

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

