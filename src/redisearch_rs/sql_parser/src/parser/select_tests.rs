use sqlparser::ast::{SetExpr, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::parser::parse;

// Basic parsing tests
#[test]
fn test_parse_simple_select() {
    let query = parse("SELECT * FROM test_idx").unwrap();
    assert_eq!(query.index_name, "test_idx");
    assert!(query.fields.is_empty()); // SELECT * means empty fields
    assert!(query.conditions.is_empty());
}

#[test]
fn test_parse_select_with_fields() {
    let query = parse("SELECT name, price, category FROM products").unwrap();
    assert_eq!(query.index_name, "products");
    assert_eq!(query.fields.len(), 3);
    assert_eq!(query.fields[0].name, "name");
    assert_eq!(query.fields[1].name, "price");
    assert_eq!(query.fields[2].name, "category");
}

#[test]
fn test_parse_select_with_alias() {
    let query = parse("SELECT name AS product_name FROM products").unwrap();
    assert_eq!(query.fields.len(), 1);
    assert_eq!(query.fields[0].name, "name");
    assert_eq!(query.fields[0].alias, Some("product_name".to_string()));
}

// Error cases
#[test]
fn test_parse_empty_query() {
    let result = parse("");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Empty"));
}

#[test]
fn test_parse_multiple_statements() {
    let result = parse("SELECT * FROM a; SELECT * FROM b");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Multiple statements"));
}

#[test]
fn test_parse_non_select_statement() {
    let result = parse("INSERT INTO idx VALUES (1)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("SELECT"));
}

#[test]
fn test_parse_union_not_supported() {
    let result = parse("SELECT * FROM a UNION SELECT * FROM b");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("Only SELECT queries"));
}

#[test]
fn test_parse_missing_from() {
    let result = parse("SELECT *");
    assert!(result.is_err());
}

#[test]
fn test_parse_multiple_tables_not_supported() {
    let result = parse("SELECT * FROM a, b");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("JOIN"));
}

#[test]
fn test_parse_join_not_supported() {
    let result = parse("SELECT * FROM a JOIN b ON a.id = b.id");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.message.contains("JOIN"));
}

#[test]
fn test_parse_distinct() {
    let query = parse("SELECT DISTINCT category FROM idx").unwrap();
    assert!(query.distinct);
    assert_eq!(query.fields.len(), 1);
}

// Case insensitivity tests
#[test]
fn test_parse_case_insensitive_keywords() {
    let query = parse("select * from idx").unwrap();
    assert_eq!(query.index_name, "idx");

    let err = parse("SELECT DISTINCT ON (category) category FROM idx").unwrap_err();
    assert!(err.message.contains("DISTINCT ON"));

    let dialect = PostgreSqlDialect {};
    let statements = Parser::parse_sql(&dialect, "SELECT * FROM (SELECT 1) AS t").unwrap();
    let Statement::Query(query) = &statements[0] else {
        panic!("Expected SELECT query");
    };
    let SetExpr::Select(select) = query.body.as_ref() else {
        panic!("Expected SELECT body");
    };

    let err = super::parse_from_clause(&select.from).unwrap_err();
    assert!(err.message.contains("Only simple table references"));
}

// Whitespace tests
#[test]
fn test_parse_extra_whitespace() {
    let query = parse("  SELECT   *   FROM   idx   WHERE   x = 1  ").unwrap();
    assert_eq!(query.index_name, "idx");
}

#[test]
fn test_parse_multiline_query() {
    let sql = "SELECT name, price
                   FROM products
                   WHERE price > 100
                   ORDER BY name";
    let query = parse(sql).unwrap();
    assert_eq!(query.index_name, "products");
    assert_eq!(query.fields.len(), 2);
    assert!(query.order_by.is_some());

    let err = parse("SELECT ABS(price) FROM products").unwrap_err();
    assert!(err.message.contains("Unsupported function in SELECT"));

    let err = parse("SELECT products.* FROM products").unwrap_err();
    assert!(err.message.contains("Unsupported SELECT item"));
}
