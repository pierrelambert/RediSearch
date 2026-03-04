# SQL Semantic Layer - Manual Test Guide

## Overview

Quick guide to manually test and visualize the new FT.SQL command in PR #3.

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
# Create products index
redis-cli FT.CREATE products SCHEMA \
  name TEXT \
  category TAG \
  price NUMERIC \
  stock NUMERIC \
  description TEXT

# Add sample products
redis-cli HSET prod1 name "Laptop Pro" category "electronics" price 1299 stock 50 description "High-performance laptop"
redis-cli HSET prod2 name "Wireless Mouse" category "electronics" price 49 stock 200 description "Ergonomic wireless mouse"
redis-cli HSET prod3 name "USB Cable" category "accessories" price 15 stock 500 description "USB-C charging cable"
redis-cli HSET prod4 name "Keyboard" category "electronics" price 129 stock 75 description "Mechanical keyboard"
redis-cli HSET prod5 name "Monitor Stand" category "accessories" price 89 stock 30 description "Adjustable monitor stand"
redis-cli HSET prod6 name "Headphones" category "electronics" price 199 stock 100 description "Noise-canceling headphones"
```

## SQL Query Examples

### 1. SELECT All

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products"

# Equivalent RQL
redis-cli FT.SEARCH products "*"
```

### 2. SELECT Specific Fields

```bash
# SQL
redis-cli FT.SQL "SELECT name, price FROM products"

# Equivalent RQL
redis-cli FT.SEARCH products "*" RETURN 2 name price
```

### 3. WHERE with Equality

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products WHERE category = 'electronics'"

# Equivalent RQL
redis-cli FT.SEARCH products "@category:{electronics}"
```

### 4. WHERE with Comparison

```bash
# SQL - Greater than
redis-cli FT.SQL "SELECT * FROM products WHERE price > 100"

# SQL - Less than
redis-cli FT.SQL "SELECT * FROM products WHERE price < 100"

# SQL - Greater or equal
redis-cli FT.SQL "SELECT * FROM products WHERE stock >= 100"
```

### 5. WHERE with BETWEEN

```bash
# SQL
redis-cli FT.SQL "SELECT * FROM products WHERE price BETWEEN 50 AND 200"

# Equivalent RQL
redis-cli FT.SEARCH products "@price:[50 200]"
```

### 6. ORDER BY

```bash
# SQL - Ascending
redis-cli FT.SQL "SELECT * FROM products ORDER BY price ASC"

# SQL - Descending
redis-cli FT.SQL "SELECT * FROM products ORDER BY price DESC"
```

### 7. LIMIT

```bash
# SQL - First 3 results
redis-cli FT.SQL "SELECT * FROM products LIMIT 3"

# SQL - With offset (pagination)
redis-cli FT.SQL "SELECT * FROM products LIMIT 3 OFFSET 2"
```

### 8. Combined Query

```bash
# SQL - Full query with all clauses
redis-cli FT.SQL "SELECT name, price FROM products WHERE price > 50 ORDER BY price DESC LIMIT 5"
```

## Translation Reference

| SQL | RQL Equivalent |
| --- | --- |
| SELECT * | (no RETURN clause) |
| SELECT field1, field2 | RETURN 2 field1 field2 |
| WHERE field = 'value' | @field:value |
| WHERE field > 100 | @field:[(100 +inf] |
| WHERE field < 100 | @field:[-inf (100] |
| WHERE field >= 100 | @field:[100 +inf] |
| WHERE field <= 100 | @field:[-inf 100] |
| WHERE field BETWEEN a AND b | @field:[a b] |
| ORDER BY field ASC | SORTBY field ASC |
| ORDER BY field DESC | SORTBY field DESC |
| LIMIT n | LIMIT 0 n |
| LIMIT n OFFSET m | LIMIT m n |

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

## Cleanup

```bash
redis-cli FT.DROPINDEX products DD
redis-cli SHUTDOWN NOSAVE
```

## Quick Reference Card

```
┌─────────────────────────────────────────────────────────────┐
│                    FT.SQL Quick Reference                   │
├─────────────────────────────────────────────────────────────┤
│ FT.SQL "SELECT * FROM idx"                                  │
│ FT.SQL "SELECT field1, field2 FROM idx"                     │
│ FT.SQL "SELECT * FROM idx WHERE price > 100"                │
│ FT.SQL "SELECT * FROM idx WHERE price BETWEEN 10 AND 100"   │
│ FT.SQL "SELECT * FROM idx ORDER BY price DESC"              │
│ FT.SQL "SELECT * FROM idx LIMIT 10"                         │
│ FT.SQL "SELECT * FROM idx LIMIT 10 OFFSET 5"                │
└─────────────────────────────────────────────────────────────┘
```