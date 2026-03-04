# SQL Semantic Layer - Manual Test Guide

## Overview

Comprehensive guide to test the FT.SQL command in RediSearch. This guide covers all Phase 1 and Phase 2 features including advanced WHERE operators, aggregation functions, GROUP BY, HAVING, and vector similarity search.

## Prerequisites

- Redis server with RediSearch module loaded
- Redis CLI

## Quick Start

```bash
# Build the module
./build.sh

# Start Redis with module
redis-server --loadmodule bin/macos-aarch64-release/search-community/redisearch.so &
sleep 3
redis-cli PING
```

## What's New: FT.SQL Command

The `FT.SQL` command allows you to query RediSearch indexes using familiar SQL syntax!

### Basic Syntax

```bash
FT.SQL "<SQL query>"
```

## Test Scenario: E-Commerce Product Catalog

### Setup Test Data

```bash
# Create products index with all field types including vector
redis-cli FT.CREATE products SCHEMA \
  name TEXT \
  category TAG INDEXMISSING \
  brand TAG \
  price NUMERIC \
  stock NUMERIC \
  rating NUMERIC \
  description TEXT \
  embedding VECTOR FLAT 6 TYPE FLOAT32 DIM 3 DISTANCE_METRIC L2

# Add sample products
redis-cli HSET prod1 name "Laptop Pro" category "electronics" brand "TechCo" price 1299 stock 50 rating 4.5 description "High-performance laptop"
redis-cli HSET prod2 name "Wireless Mouse" category "electronics" brand "TechCo" price 49 stock 200 rating 4.2 description "Ergonomic wireless mouse"
redis-cli HSET prod3 name "USB Cable" category "accessories" brand "ConnectX" price 15 stock 500 rating 3.8 description "USB-C charging cable"
redis-cli HSET prod4 name "Keyboard" category "electronics" brand "TypeMaster" price 129 stock 75 rating 4.7 description "Mechanical keyboard"
redis-cli HSET prod5 name "Monitor Stand" category "accessories" brand "DeskPro" price 89 stock 30 rating 4.0 description "Adjustable monitor stand"
redis-cli HSET prod6 name "Headphones" category "electronics" brand "AudioMax" price 199 stock 100 rating 4.8 description "Noise-canceling headphones"
redis-cli HSET prod7 name "Phone Case" brand "PhoneGuard" price 25 stock 1000 rating 3.5 description "Protective phone case"

# Add vector embeddings (as binary blobs in practice, shown conceptually)
# redis-cli HSET prod1 embedding "\x00\x00\x80\x3f..." (binary float32 data)
```

## SQL Query Examples

### Phase 1: Basic Queries

#### 1. SELECT All

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products"

# Equivalent RQL
redis-cli FT.SEARCH products "*"
```

#### 2. SELECT Specific Fields

```bash
# SQL
redis-cli FT.SQL "SELECT name, price FROM products"

# Equivalent RQL
redis-cli FT.SEARCH products "*" RETURN 2 name price
```

#### 3. SELECT with Alias

```bash
# SQL
redis-cli FT.SQL "SELECT name, price AS cost FROM products"
```

#### 4. WHERE with Equality

```bash
# SQL - TAG field
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics'"

# SQL - NUMERIC field
redis-cli FT.SQL "SELECT * FROM products WHERE price = 49"

# Equivalent RQL
redis-cli FT.SEARCH products "@category:{electronics}"
```

#### 5. WHERE with Inequality

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products WHERE category != 'accessories'"

# Equivalent RQL
redis-cli FT.SEARCH products "-@category:{accessories}"
```

#### 6. WHERE with Comparison

```bash
# SQL - Greater than
redis-cli FT.SQL "SELECT * FROM products WHERE price > 100"

# SQL - Less than
redis-cli FT.SQL "SELECT * FROM products WHERE price < 100"

# SQL - Greater or equal
redis-cli FT.SQL "SELECT * FROM products WHERE stock >= 100"

# SQL - Less or equal
redis-cli FT.SQL "SELECT * FROM products WHERE rating <= 4.0"
```

#### 7. WHERE with BETWEEN

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products WHERE price BETWEEN 50 AND 200"

# Equivalent RQL
redis-cli FT.SEARCH products "@price:[50 200]"
```

#### 8. ORDER BY

```bash
# SQL - Ascending
redis-cli FT.SQL "SELECT * FROM products ORDER BY price ASC"

# SQL - Descending
redis-cli FT.SQL "SELECT * FROM products ORDER BY price DESC"
```

#### 9. LIMIT and OFFSET

```bash
# SQL - First 3 results
redis-cli FT.SQL "SELECT * FROM products LIMIT 3"

# SQL - With offset (pagination)
redis-cli FT.SQL "SELECT * FROM products LIMIT 3 OFFSET 2"
```

#### 10. Combined Query

```bash
# SQL - Full query with all clauses
redis-cli FT.SQL "SELECT name, price FROM products WHERE price > 50 ORDER BY price DESC LIMIT 5"
```

### Phase 2: Advanced WHERE Operators

#### 11. IN / NOT IN

```bash
# SQL - IN (multiple values)
redis-cli FT.SQL "SELECT * FROM products WHERE category IN ('electronics', 'accessories')"

# SQL - NOT IN
redis-cli FT.SQL "SELECT * FROM products WHERE brand NOT IN ('TechCo', 'AudioMax')"

# Equivalent RQL
redis-cli FT.SEARCH products "@category:{electronics|accessories}"
```

#### 12. LIKE (Pattern Matching)

```bash
# SQL - Prefix match (starts with)
redis-cli FT.SQL "SELECT * FROM products WHERE name LIKE 'Lap%'"

# SQL - Suffix match (ends with)
redis-cli FT.SQL "SELECT * FROM products WHERE name LIKE '%board'"

# SQL - Contains
redis-cli FT.SQL "SELECT * FROM products WHERE name LIKE '%less%'"

# Equivalent RQL
redis-cli FT.SEARCH products "@name:Lap*"
redis-cli FT.SEARCH products "@name:*board"
redis-cli FT.SEARCH products "@name:*less*"
```

#### 13. IS NULL / IS NOT NULL

```bash
# SQL - Find products without category (requires INDEXMISSING attribute)
redis-cli FT.SQL "SELECT * FROM products WHERE category IS NULL"

# SQL - Find products with category
redis-cli FT.SQL "SELECT * FROM products WHERE category IS NOT NULL"

# Equivalent RQL (FT.AGGREGATE)
redis-cli FT.AGGREGATE products "*" FILTER "ismissing(@category)"
```

> Note: IS NULL only works on fields defined with the INDEXMISSING attribute.See the Limitations section for details.

#### 14. AND / OR / NOT

```bash
# SQL - AND (multiple conditions)
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics' AND price > 100"

# SQL - OR (alternative conditions)
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics' OR category = 'accessories'"

# SQL - NOT (negation)
redis-cli FT.SQL "SELECT * FROM products WHERE NOT (price > 500)"

# SQL - Complex boolean expression
redis-cli FT.SQL "SELECT * FROM products WHERE (category = 'electronics' AND price > 100) OR brand = 'DeskPro'"

# Equivalent RQL
redis-cli FT.SEARCH products "(@category:{electronics} @price:[(100 +inf])"
redis-cli FT.SEARCH products "(@category:{electronics}) | (@category:{accessories})"
```

### Phase 2: SELECT DISTINCT

#### 15. DISTINCT Values

```bash
# SQL - Unique categories
redis-cli FT.SQL "SELECT DISTINCT category FROM products"

# SQL - Multiple distinct fields
redis-cli FT.SQL "SELECT DISTINCT category, brand FROM products"
```

### Phase 2: Aggregation Functions

#### 16. Basic Aggregates

```bash
# COUNT - Total number of products
redis-cli FT.SQL "SELECT COUNT(*) FROM products"

# COUNT with condition
redis-cli FT.SQL "SELECT COUNT(*) FROM products WHERE category = 'electronics'"

# SUM - Total stock
redis-cli FT.SQL "SELECT SUM(stock) FROM products"

# AVG - Average price
redis-cli FT.SQL "SELECT AVG(price) FROM products"

# MIN / MAX - Price range
redis-cli FT.SQL "SELECT MIN(price), MAX(price) FROM products"

# Multiple aggregates
redis-cli FT.SQL "SELECT COUNT(*), SUM(price), AVG(price), MIN(price), MAX(price) FROM products"
```

#### 17. COUNT_DISTINCT

```bash
# SQL - Count unique categories
redis-cli FT.SQL "SELECT COUNT_DISTINCT(category) FROM products"

# SQL - Count unique brands per category
redis-cli FT.SQL "SELECT category, COUNT_DISTINCT(brand) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 0 REDUCE COUNT_DISTINCT 1 @category AS unique_categories
```

#### 18. COUNT_DISTINCTISH (Approximate)

```bash
# SQL - Approximate unique count (faster for large datasets)
redis-cli FT.SQL "SELECT COUNT_DISTINCTISH(brand) FROM products"

# Equivalent RQL (uses probabilistic counting)
redis-cli FT.AGGREGATE products "*" GROUPBY 0 REDUCE COUNT_DISTINCTISH 1 @brand AS approx_brands
```

#### 19. STDDEV (Standard Deviation)

```bash
# SQL - Price standard deviation
redis-cli FT.SQL "SELECT STDDEV(price) FROM products"

# SQL - Per-category standard deviation
redis-cli FT.SQL "SELECT category, STDDEV(price) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 0 REDUCE STDDEV 1 @price AS price_stddev
```

#### 20. QUANTILE (Percentile)

```bash
# SQL - 99th percentile price
redis-cli FT.SQL "SELECT QUANTILE(price, 0.99) FROM products"

# SQL - Median (50th percentile)
redis-cli FT.SQL "SELECT QUANTILE(price, 0.5) FROM products"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 0 REDUCE QUANTILE 2 @price 0.99 AS p99_price
```

#### 21. TOLIST (Collect Values)

```bash
# SQL - List all product names per category
redis-cli FT.SQL "SELECT category, TOLIST(name) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE TOLIST 1 @name AS names
```

#### 22. FIRST_VALUE (First by Sort Order)

```bash
# SQL - Cheapest product name per category
redis-cli FT.SQL "SELECT category, FIRST_VALUE(name, price) FROM products GROUP BY category"

# SQL - Most expensive product per category
redis-cli FT.SQL "SELECT category, FIRST_VALUE(name, -price) FROM products GROUP BY category"

# Equivalent RQL (negative sort key for descending)
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE FIRST_VALUE 2 @name @price AS cheapest
```

#### 23. RANDOM_SAMPLE

```bash
# SQL - Random 3 products per category
redis-cli FT.SQL "SELECT category, RANDOM_SAMPLE(name, 3) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE RANDOM_SAMPLE 2 @name 3 AS sample_products
```

#### 24. HLL (HyperLogLog)

```bash
# SQL - Create HLL for unique user counting
redis-cli FT.SQL "SELECT HLL(brand) FROM products"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 0 REDUCE HLL 1 @brand AS hll_brands
```

#### 25. HLL_SUM (Merge HyperLogLogs)

```bash
# SQL - Merge HLL values (for distributed counting)
redis-cli FT.SQL "SELECT HLL_SUM(hll_field) FROM shards"

# Equivalent RQL
redis-cli FT.AGGREGATE shards "*" GROUPBY 0 REDUCE HLL_SUM 1 @hll_field AS merged_hll
```

### Phase 2: GROUP BY

#### 26. Simple GROUP BY

```bash
# SQL - Count products per category
redis-cli FT.SQL "SELECT category, COUNT(*) FROM products GROUP BY category"

# SQL - Average price per category
redis-cli FT.SQL "SELECT category, AVG(price) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE COUNT 0 AS count
```

#### 27. Multiple GROUP BY Fields

```bash
# SQL - Group by category and brand
redis-cli FT.SQL "SELECT category, brand, COUNT(*) FROM products GROUP BY category, brand"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 2 @category @brand REDUCE COUNT 0 AS count
```

#### 28. GROUP BY with Multiple Aggregates

```bash
# SQL - Multiple aggregates per group
redis-cli FT.SQL "SELECT category, COUNT(*), AVG(price), MAX(rating) FROM products GROUP BY category"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category \
  REDUCE COUNT 0 AS count \
  REDUCE AVG 1 @price AS avg_price \
  REDUCE MAX 1 @rating AS max_rating
```

### Phase 2: HAVING

#### 29. HAVING with COUNT

```bash
# SQL - Categories with more than 3 products
redis-cli FT.SQL "SELECT category, COUNT(*) AS cnt FROM products GROUP BY category HAVING COUNT(*) > 3"

# SQL - Using alias in HAVING
redis-cli FT.SQL "SELECT category, COUNT(*) AS cnt FROM products GROUP BY category HAVING cnt > 3"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE COUNT 0 AS cnt FILTER "@cnt > 3"
```

#### 30. HAVING with AVG

```bash
# SQL - Categories with average price over 100
redis-cli FT.SQL "SELECT category, AVG(price) AS avg_price FROM products GROUP BY category HAVING AVG(price) > 100"

# Equivalent RQL
redis-cli FT.AGGREGATE products "*" GROUPBY 1 @category REDUCE AVG 1 @price AS avg_price FILTER "@avg_price > 100"
```

#### 31. HAVING with Multiple Conditions

```bash
# SQL - Complex HAVING
redis-cli FT.SQL "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price FROM products GROUP BY category HAVING COUNT(*) >= 2 AND AVG(price) < 500"
```

### Phase 2: Vector Similarity Search

#### 32. L2 Distance (Euclidean)

```bash
# SQL - Find 10 nearest products by L2 distance
redis-cli FT.SQL "SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10"

# Equivalent RQL
redis-cli FT.SEARCH products "*=>[KNN 10 @embedding $vec]" PARAMS 2 vec "\x00\x00\x80\x3e..." DIALECT 2
```

#### 33. Cosine Similarity

```bash
# SQL - Find similar products by cosine similarity
redis-cli FT.SQL "SELECT * FROM products ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10"

# Equivalent RQL (requires COSINE distance metric in schema)
redis-cli FT.SEARCH products "*=>[KNN 10 @embedding $vec]" PARAMS 2 vec "..." DIALECT 2
```

#### 34. Inner Product

```bash
# SQL - Find products by inner product (for normalized vectors)
redis-cli FT.SQL "SELECT * FROM products ORDER BY embedding <#> '[0.1, 0.2, 0.3]' LIMIT 10"
```

#### 35. Hybrid Search (Filter + Vector)

```bash
# SQL - Filter first, then vector search
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 5"

# SQL - Multiple filters with vector
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics' AND price < 500 ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10"

# With weights (text + vector)
redis-cli FT.SQL "SELECT * FROM products WHERE description LIKE '%laptop%' ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10 OPTION(vector_weight=0.7, text_weight=0.3)"
```

### Phase 2: Multiple ORDER BY (FT.AGGREGATE only)

#### 36. Multi-Column Sorting

```bash
# SQL - Sort by category ascending, then price descending
redis-cli FT.SQL "SELECT * FROM products ORDER BY category ASC, price DESC"

# Equivalent RQL (FT.AGGREGATE)
redis-cli FT.AGGREGATE products "*" SORTBY 4 @category ASC @price DESC
```

> Note: Multiple ORDER BY columns require FT.AGGREGATE. FT.SEARCH only supports single-column sorting.

## Translation Reference

### Basic Operations

| SQL | RQL Equivalent |
| --- | --- |
| SELECT * | (no RETURN clause) |
| SELECT field1, field2 | RETURN 2 field1 field2 |
| SELECT field AS alias | RETURN 1 field AS alias |
| SELECT DISTINCT field | GROUPBY 1 @field |

### WHERE Operators

| SQL | RQL Equivalent |
| --- | --- |
| WHERE field = 'value' | @field:{value} (TAG) or @field:value (TEXT) |
| WHERE field != 'value' | -@field:{value} |
| WHERE field > 100 | @field:[(100 +inf] |
| WHERE field >= 100 | @field:[100 +inf] |
| WHERE field < 100 | @field:[-inf (100] |
| WHERE field <= 100 | @field:[-inf 100] |
| WHERE field BETWEEN a AND b | @field:[a b] |
| WHERE field IN ('a', 'b') | @field:{a |
| WHERE field NOT IN ('a', 'b') | -@field:{a |
| WHERE field LIKE 'Lap%' | @field:Lap* |
| WHERE field LIKE '%end' | @field:*end |
| WHERE field LIKE '%mid%' | @field:mid |
| WHERE field IS NULL | FILTER "ismissing(@field)" |
| WHERE field IS NOT NULL | FILTER "!ismissing(@field)" |

### Boolean Operators

| SQL | RQL Equivalent |
| --- | --- |
| WHERE a AND b | (a b) |
| WHERE a OR b | (a) |
| WHERE NOT (a) | -(a) |

### Sorting and Pagination

| SQL | RQL Equivalent |
| --- | --- |
| ORDER BY field ASC | SORTBY field ASC |
| ORDER BY field DESC | SORTBY field DESC |
| ORDER BY a ASC, b DESC | SORTBY 4 @a ASC @b DESC (FT.AGGREGATE only) |
| LIMIT n | LIMIT 0 n |
| LIMIT n OFFSET m | LIMIT m n |

### Aggregation Functions

| SQL | RQL Equivalent |
| --- | --- |
| COUNT(*) | REDUCE COUNT 0 |
| SUM(field) | REDUCE SUM 1 @field |
| AVG(field) | REDUCE AVG 1 @field |
| MIN(field) | REDUCE MIN 1 @field |
| MAX(field) | REDUCE MAX 1 @field |
| COUNT_DISTINCT(field) | REDUCE COUNT_DISTINCT 1 @field |
| COUNT_DISTINCTISH(field) | REDUCE COUNT_DISTINCTISH 1 @field |
| STDDEV(field) | REDUCE STDDEV 1 @field |
| QUANTILE(field, 0.99) | REDUCE QUANTILE 2 @field 0.99 |
| TOLIST(field) | REDUCE TOLIST 1 @field |
| FIRST_VALUE(f1, f2) | REDUCE FIRST_VALUE 2 @f1 @f2 |
| RANDOM_SAMPLE(field, n) | REDUCE RANDOM_SAMPLE 2 @field n |
| HLL(field) | REDUCE HLL 1 @field |
| HLL_SUM(field) | REDUCE HLL_SUM 1 @field |

### Grouping

| SQL | RQL Equivalent |
| --- | --- |
| GROUP BY field | GROUPBY 1 @field |
| GROUP BY a, b | GROUPBY 2 @a @b |
| HAVING condition | FILTER "condition" (after GROUPBY) |

### Vector Search

| SQL Operator | Distance Metric | RQL |
| --- | --- | --- |
| <-> | L2 (Euclidean) | *=>[KNN n @field $vec] |
| <=> | Cosine | *=>[KNN n @field $vec] |
| <#> | Inner Product | *=>[KNN n @field $vec] |

## Limitations

### 1. Multiple ORDER BY Columns

**Limitation**: Multiple `ORDER BY` columns only work with `FT.AGGREGATE`.

```bash
# Works (single column uses FT.SEARCH)
redis-cli FT.SQL "SELECT * FROM products ORDER BY price DESC"

# Works (multiple columns use FT.AGGREGATE)
redis-cli FT.SQL "SELECT * FROM products ORDER BY category ASC, price DESC"

# Note: FT.SEARCH rejects multiple SORTBY fields with an error
```

### 2. IS NULL Requires INDEXMISSING

**Limitation**: `IS NULL` / `IS NOT NULL` only works on fields with the `INDEXMISSING` attribute.

```bash
# Schema must include INDEXMISSING
redis-cli FT.CREATE idx SCHEMA category TAG INDEXMISSING

# Then IS NULL works
redis-cli FT.SQL "SELECT * FROM idx WHERE category IS NULL"

# Without INDEXMISSING, IS NULL always returns empty results
```

### 3. TOLIST, FIRST_VALUE, RANDOM_SAMPLE Require GROUP BY

**Limitation**: These aggregate functions must be used with `GROUP BY`.

```bash
# Error - no GROUP BY
redis-cli FT.SQL "SELECT TOLIST(name) FROM products"

# Works - with GROUP BY
redis-cli FT.SQL "SELECT category, TOLIST(name) FROM products GROUP BY category"
```

### 4. Wildcard Patterns in LIKE

**Limitation**: The `_` (single character) wildcard may have limited support.

```bash
# Works well
redis-cli FT.SQL "SELECT * FROM products WHERE name LIKE 'Lap%'"

# May have limitations
redis-cli FT.SQL "SELECT * FROM products WHERE name LIKE 'Lap___'"
```

### 5. Nested Subqueries

**Limitation**: Subqueries are not supported.

```bash
# Not supported
redis-cli FT.SQL "SELECT * FROM products WHERE price > (SELECT AVG(price) FROM products)"
```

### 6. JOIN Operations

**Limitation**: JOINs are not supported (single index per query).

```bash
# Not supported
redis-cli FT.SQL "SELECT * FROM products p JOIN categories c ON p.category = c.id"
```

## Error Handling

### Invalid SQL Syntax

```bash
redis-cli FT.SQL "SELEC * FROM products"
# Error: SQL syntax error
```

### Missing Index

```bash
redis-cli FT.SQL "SELECT * FROM nonexistent"
# Error: Index not found
```

### Invalid Field

```bash
redis-cli FT.SQL "SELECT * FROM products WHERE invalid_field > 100"
# Error or empty results
```

### Unsupported Feature

```bash
redis-cli FT.SQL "SELECT * FROM products JOIN other ON products.id = other.pid"
# Error: JOIN not supported
```

## Cleanup

```bash
redis-cli FT.DROPINDEX products DD
redis-cli SHUTDOWN NOSAVE
```

## Quick Reference Card

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         FT.SQL Quick Reference                              │
├─────────────────────────────────────────────────────────────────────────────┤
│ BASIC QUERIES                                                               │
│   FT.SQL "SELECT * FROM idx"                                                │
│   FT.SQL "SELECT name, price FROM idx"                                      │
│   FT.SQL "SELECT name AS product_name FROM idx"                             │
│   FT.SQL "SELECT DISTINCT category FROM idx"                                │
├─────────────────────────────────────────────────────────────────────────────┤
│ WHERE CLAUSES                                                               │
│   FT.SQL "SELECT * FROM idx WHERE price > 100"                              │
│   FT.SQL "SELECT * FROM idx WHERE price BETWEEN 10 AND 100"                 │
│   FT.SQL "SELECT * FROM idx WHERE category IN ('a', 'b')"                   │
│   FT.SQL "SELECT * FROM idx WHERE name LIKE 'Lap%'"                         │
│   FT.SQL "SELECT * FROM idx WHERE field IS NULL"                            │
│   FT.SQL "SELECT * FROM idx WHERE a = 1 AND b > 2"                          │
│   FT.SQL "SELECT * FROM idx WHERE a = 1 OR b = 2"                           │
├─────────────────────────────────────────────────────────────────────────────┤
│ SORTING & PAGINATION                                                        │
│   FT.SQL "SELECT * FROM idx ORDER BY price DESC"                            │
│   FT.SQL "SELECT * FROM idx ORDER BY cat ASC, price DESC"                   │
│   FT.SQL "SELECT * FROM idx LIMIT 10"                                       │
│   FT.SQL "SELECT * FROM idx LIMIT 10 OFFSET 20"                             │
├─────────────────────────────────────────────────────────────────────────────┤
│ AGGREGATION                                                                 │
│   FT.SQL "SELECT COUNT(*), AVG(price) FROM idx"                             │
│   FT.SQL "SELECT COUNT_DISTINCT(category) FROM idx"                         │
│   FT.SQL "SELECT QUANTILE(price, 0.95) FROM idx"                            │
│   FT.SQL "SELECT category, COUNT(*) FROM idx GROUP BY category"             │
│   FT.SQL "SELECT cat, TOLIST(name) FROM idx GROUP BY cat"                   │
├─────────────────────────────────────────────────────────────────────────────┤
│ HAVING                                                                      │
│   FT.SQL "SELECT cat, COUNT(*) FROM idx GROUP BY cat HAVING COUNT(*) > 5"   │
├─────────────────────────────────────────────────────────────────────────────┤
│ VECTOR SEARCH                                                               │
│   FT.SQL "SELECT * FROM idx ORDER BY emb <-> '[0.1,0.2]' LIMIT 10"  (L2)    │
│   FT.SQL "SELECT * FROM idx ORDER BY emb <=> '[0.1,0.2]' LIMIT 10"  (Cos)   │
│   FT.SQL "SELECT * FROM idx ORDER BY emb <#> '[0.1,0.2]' LIMIT 10"  (IP)    │
│   FT.SQL "SELECT * FROM idx WHERE cat='x' ORDER BY emb <-> '[...]' LIMIT 5" │
└─────────────────────────────────────────────────────────────────────────────┘
```