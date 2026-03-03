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

### Phase 1: MVP (Core Queries)

```sql
-- Basic SELECT
SELECT * FROM idx
SELECT field1, field2 FROM idx

-- WHERE clause (equality, comparison)
WHERE field = 'value'
WHERE field > 100
WHERE field < 100
WHERE field >= 100
WHERE field <= 100
WHERE field BETWEEN 10 AND 100

-- Sorting and pagination
ORDER BY field ASC
ORDER BY field DESC
LIMIT n
LIMIT n OFFSET m
```

### Phase 2: Enhanced (Boolean Logic & Aggregations)

```sql
-- Pattern matching
WHERE field LIKE '%word%'
WHERE field IN ('a', 'b', 'c')

-- Boolean logic
WHERE field1 = 'a' AND field2 > 10
WHERE field1 = 'a' OR field2 = 'b'
WHERE NOT field = 'value'

-- Aggregations
SELECT COUNT(*) FROM idx
SELECT AVG(price), SUM(quantity) FROM idx GROUP BY category
SELECT field, COUNT(*) FROM idx GROUP BY field
SELECT MAX(price), MIN(price) FROM idx WHERE category = 'electronics'
```

### Phase 3: Advanced (Multi-Index & Subqueries)

```sql
-- JOIN (translated to multiple queries + client-side merge)
SELECT a.*, b.name FROM idx1 a JOIN idx2 b ON a.ref = b.id

-- HAVING clause
SELECT category, AVG(price) FROM idx GROUP BY category HAVING AVG(price) > 100

-- Subqueries (limited support, document constraints)
SELECT * FROM idx WHERE category IN (SELECT DISTINCT category FROM idx WHERE featured = 1)
```

**Note**: JOINs and subqueries are complex features that require careful design. Phase 3 may implement these as multi-query patterns with documented limitations.

---

## 3. Translation Rules

### SELECT Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `SELECT *` | No RETURN clause | Returns all indexed fields |
| `SELECT a, b` | `RETURN 2 a b` | Explicit field selection |
| `SELECT COUNT(*)` | → `FT.AGGREGATE ... REDUCE COUNT 0` | Switches to aggregate |
| `SELECT AVG(field)` | → `FT.AGGREGATE ... REDUCE AVG 1 @field` | |
| `SELECT SUM(field)` | → `FT.AGGREGATE ... REDUCE SUM 1 @field` | |
| `SELECT MIN(field)` | → `FT.AGGREGATE ... REDUCE MIN 1 @field` | |
| `SELECT MAX(field)` | → `FT.AGGREGATE ... REDUCE MAX 1 @field` | |

### FROM Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `FROM idx` | Index name parameter | Required, must exist |
| `FROM idx AS alias` | Alias tracked internally | For JOIN support |

### WHERE Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `field = 'value'` | `@field:value` | Exact match (TAG field) |
| `field = 'hello world'` | `@field:(hello world)` | Multi-word value |
| `field > 100` | `@field:[(100 +inf]` | Exclusive lower bound |
| `field >= 100` | `@field:[100 +inf]` | Inclusive lower bound |
| `field < 100` | `@field:[-inf (100]` | Exclusive upper bound |
| `field <= 100` | `@field:[-inf 100]` | Inclusive upper bound |
| `field BETWEEN a AND b` | `@field:[a b]` | Inclusive range |
| `field LIKE '%word%'` | `@field:*word*` | Contains pattern |
| `field LIKE 'word%'` | `@field:word*` | Prefix pattern |
| `field LIKE '%word'` | `@field:*word` | Suffix pattern |
| `field IN ('a','b','c')` | `@field:(a\|b\|c)` | OR alternatives |
| `a AND b` | `(@a) (@b)` | Implicit intersection |
| `a OR b` | `(@a) \| (@b)` | Union operator |
| `NOT condition` | `-(@condition)` | Negation prefix |
| `field IS NULL` | `-@field:*` | Field not present |
| `field IS NOT NULL` | `@field:*` | Field exists |

### ORDER BY Clause

| SQL | RQL Translation | Notes |
|-----|-----------------|-------|
| `ORDER BY field` | `SORTBY field` | Default ASC |
| `ORDER BY field ASC` | `SORTBY field ASC` | Explicit ascending |
| `ORDER BY field DESC` | `SORTBY field DESC` | Descending order |
| `ORDER BY a, b` | `SORTBY a ASC b ASC` | Multi-field sort |

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

**Cache Configuration**:
- Default capacity: 1000 queries
- Default TTL: 300 seconds (5 minutes)
- Eviction: LRU when at capacity

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
    assert_eq!(result.query_string, "@status:active");
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
#define SQL_LAYER_ENABLED_DEFAULT false

// Runtime toggle
FT.CONFIG SET SQL_LAYER_ENABLED true|false
```

### Documentation Updates

1. **New page**: `docs/commands/FT.SQL.md` - Command reference
2. **Examples page**: Common SQL queries and their RQL equivalents
3. **Migration guide**: For users coming from SQL databases
4. **Limitations page**: What's not supported and why

### Rollout Phases

**Alpha** (internal testing):
- Feature flag off by default
- Limited to Phase 1 SQL subset
- Extensive logging for debugging

**Beta** (opt-in users):
- Feature flag configurable
- Phase 1 + Phase 2 features
- Performance monitoring

**GA** (general availability):
- Feature flag on by default
- Full Phase 1-2 support
- Phase 3 as experimental

---

## 10. Open Questions / Future Work

### Full-Text Search Syntax

**Question**: How to express full-text search in SQL?

**Option A**: MATCH...AGAINST (MySQL-style)
```sql
SELECT * FROM idx WHERE MATCH(body) AGAINST('search terms')
```

**Option B**: FTS function
```sql
SELECT * FROM idx WHERE FTS(body, 'search terms')
```

**Option C**: Special operator
```sql
SELECT * FROM idx WHERE body CONTAINS 'search terms'
```

**Recommendation**: Defer to Phase 3; use standard equality for now, which maps to exact TAG matching.

### Vector Search Syntax

**Question**: How to express vector similarity in SQL?

**Potential syntax**:
```sql
-- Option A: SIMILAR TO operator
SELECT * FROM idx WHERE embedding SIMILAR TO [0.1, 0.2, ...] LIMIT 10

-- Option B: KNN function
SELECT * FROM idx WHERE KNN(embedding, [0.1, 0.2, ...], 10)

-- Option C: Distance function in ORDER BY
SELECT * FROM idx ORDER BY VECTOR_DISTANCE(embedding, [0.1, 0.2, ...]) LIMIT 10
```

**Recommendation**: Requires research into SQL extensions for vector databases. Defer to Phase 3.

### Geospatial Queries

**Question**: How to express geo queries in SQL?

```sql
-- Radius search
SELECT * FROM idx WHERE DISTANCE(location, POINT(-122.4, 37.8)) < 10 km

-- Bounding box
SELECT * FROM idx WHERE location WITHIN BOX(...)
```

**Recommendation**: Follow PostGIS conventions where applicable. Defer to Phase 3.

### Schema Discovery

**Question**: Should we support SHOW/DESCRIBE commands?

```sql
SHOW INDEXES;
DESCRIBE idx;
SHOW COLUMNS FROM idx;
```

**Recommendation**: Nice-to-have for Phase 2. Maps to `FT._LIST` and `FT.INFO`.

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
FT.SEARCH products "@category:electronics @price:[100 500]"
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
FT.AGGREGATE users "@status:active"
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
    "((@category:tech) | (@category:science)) -(@status:draft) @published_date:[2024-01-01 +inf]"
```

---

## Appendix B: Unsupported SQL Features

| Feature | Reason | Alternative |
|---------|--------|-------------|
| `JOIN` (Phase 1-2) | Multiple index queries not supported | Use application-level joins |
| `UNION` | No equivalent in RQL | Run multiple queries |
| `DISTINCT` | Implicit in GROUP BY | Use `GROUP BY field` |
| `CASE WHEN` | No conditional expressions in RQL | Use application logic |
| `Subqueries` | Query composition not supported | Flatten to single query |
| `Window Functions` | No OVER/PARTITION BY | Use FT.AGGREGATE |
| `HAVING` (Phase 1) | Filter after GROUP BY | Phase 2 feature |
| `OUTER JOIN` | Only intersection semantics | Not planned |

---

## Appendix C: Configuration Options

| Config Key | Type | Default | Description |
|------------|------|---------|-------------|
| `SQL_LAYER_ENABLED` | bool | false | Enable FT.SQL command |
| `SQL_CACHE_SIZE` | int | 1000 | Max cached translations |
| `SQL_CACHE_TTL` | int | 300 | Cache TTL in seconds |
| `SQL_MAX_QUERY_SIZE` | int | 65536 | Max SQL query size (bytes) |
| `SQL_TRANSLATION_TIMEOUT` | int | 100 | Translation timeout (ms) |

