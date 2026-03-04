/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use sqlparser::ast::{
    BinaryOperator, Expr, Offset, OffsetRows, OrderByExpr, SelectItem, SetExpr, Statement,
    TableFactor,
};
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;

use crate::ast::{Condition, Limit, OrderBy, SelectQuery, SortDirection, Value};
use crate::error::SqlError;

/// Parses a SQL query string into our internal AST representation.
pub fn parse(sql: &str) -> Result<SelectQuery, SqlError> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, sql)?;

    if statements.is_empty() {
        return Err(SqlError::syntax("Empty query"));
    }

    if statements.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple statements are not supported",
        ));
    }

    let statement = statements.into_iter().next().unwrap();
    parse_statement(statement)
}

fn parse_statement(statement: Statement) -> Result<SelectQuery, SqlError> {
    match statement {
        Statement::Query(query) => {
            let select = match *query.body {
                SetExpr::Select(select) => select,
                _ => {
                    return Err(SqlError::unsupported(
                        "Only SELECT queries are supported in Phase 1",
                    ));
                }
            };

            // Parse fields from SELECT clause
            let fields = parse_select_items(&select.projection)?;

            // Parse FROM clause
            let index_name = parse_from_clause(&select.from)?;

            // Parse WHERE clause
            let conditions = if let Some(selection) = select.selection {
                parse_where_clause(&selection)?
            } else {
                Vec::new()
            };

            // Parse ORDER BY clause
            let order_by = if let Some(ref ob) = query.order_by {
                parse_order_by(&ob.exprs)?
            } else {
                None
            };

            // Parse LIMIT/OFFSET clause
            let limit = parse_limit(query.limit.as_ref(), query.offset.as_ref())?;

            Ok(SelectQuery {
                fields,
                index_name,
                conditions,
                order_by,
                limit,
            })
        }
        _ => Err(SqlError::unsupported(
            "Only SELECT statements are supported in Phase 1",
        )),
    }
}

fn parse_select_items(items: &[SelectItem]) -> Result<Vec<String>, SqlError> {
    let mut fields = Vec::new();

    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                // SELECT * - return empty vec to indicate all fields
                return Ok(Vec::new());
            }
            SelectItem::UnnamedExpr(expr) => {
                let field_name = extract_identifier(expr)?;
                fields.push(field_name);
            }
            SelectItem::ExprWithAlias { expr, .. } => {
                let field_name = extract_identifier(expr)?;
                fields.push(field_name);
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "Unsupported SELECT item: {item:?}"
                )));
            }
        }
    }

    Ok(fields)
}

fn parse_from_clause(from: &[sqlparser::ast::TableWithJoins]) -> Result<String, SqlError> {
    if from.is_empty() {
        return Err(SqlError::syntax("FROM clause is required"));
    }

    if from.len() > 1 {
        return Err(SqlError::unsupported("JOINs are not supported in Phase 1"));
    }

    let table = &from[0];
    if !table.joins.is_empty() {
        return Err(SqlError::unsupported("JOINs are not supported in Phase 1"));
    }

    match &table.relation {
        TableFactor::Table { name, .. } => Ok(name.to_string()),
        _ => Err(SqlError::unsupported(
            "Only simple table references are supported",
        )),
    }
}

fn parse_where_clause(expr: &Expr) -> Result<Vec<Condition>, SqlError> {
    let mut conditions = Vec::new();
    parse_expression(expr, &mut conditions)?;
    Ok(conditions)
}

fn parse_expression(expr: &Expr, conditions: &mut Vec<Condition>) -> Result<(), SqlError> {
    match expr {
        Expr::BinaryOp { left, op, right } => parse_binary_op(left, op, right, conditions),
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            let field = extract_identifier(expr)?;
            let low_val = extract_value(low)?;
            let high_val = extract_value(high)?;
            conditions.push(Condition::Between {
                field,
                low: low_val,
                high: high_val,
            });
            Ok(())
        }
        Expr::Between { negated: true, .. } => Err(SqlError::unsupported(
            "NOT BETWEEN is not supported in Phase 1",
        )),
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            let field = extract_identifier(expr)?;
            let values: Vec<Value> = list
                .iter()
                .map(extract_value)
                .collect::<Result<Vec<_>, _>>()?;

            if values.is_empty() {
                return Err(SqlError::syntax("IN clause requires at least one value"));
            }

            conditions.push(Condition::In {
                field,
                values,
                negated: *negated,
            });
            Ok(())
        }
        Expr::Nested(inner) => parse_expression(inner, conditions),
        _ => Err(SqlError::unsupported(format!(
            "Unsupported expression type: {expr:?}"
        ))),
    }
}

fn parse_binary_op(
    left: &Expr,
    op: &BinaryOperator,
    right: &Expr,
    conditions: &mut Vec<Condition>,
) -> Result<(), SqlError> {
    match op {
        BinaryOperator::Eq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::Equals { field, value });
            Ok(())
        }
        BinaryOperator::Gt => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::GreaterThan { field, value });
            Ok(())
        }
        BinaryOperator::GtEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::GreaterThanOrEqual { field, value });
            Ok(())
        }
        BinaryOperator::Lt => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::LessThan { field, value });
            Ok(())
        }
        BinaryOperator::LtEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::LessThanOrEqual { field, value });
            Ok(())
        }
        BinaryOperator::And => {
            // For AND, recursively parse both sides
            parse_expression(left, conditions)?;
            parse_expression(right, conditions)?;
            Ok(())
        }
        _ => Err(SqlError::unsupported(format!(
            "Operator {op:?} is not supported in Phase 1"
        ))),
    }
}

fn extract_identifier(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        Expr::CompoundIdentifier(parts) => {
            // For compound identifiers like table.column, just use the last part
            Ok(parts.last().map(|i| i.value.clone()).unwrap_or_default())
        }
        _ => Err(SqlError::translation(format!(
            "Expected identifier, got: {expr:?}"
        ))),
    }
}

fn extract_value(expr: &Expr) -> Result<Value, SqlError> {
    match expr {
        Expr::Value(val) => match val {
            sqlparser::ast::Value::Number(n, _) => {
                let num: f64 = n
                    .parse()
                    .map_err(|_| SqlError::translation(format!("Invalid number: {n}")))?;
                Ok(Value::Number(num))
            }
            sqlparser::ast::Value::SingleQuotedString(s)
            | sqlparser::ast::Value::DoubleQuotedString(s) => Ok(Value::String(s.clone())),
            _ => Err(SqlError::unsupported(format!(
                "Unsupported value type: {val:?}"
            ))),
        },
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Minus,
            expr,
        } => {
            // Handle negative numbers
            if let Expr::Value(sqlparser::ast::Value::Number(n, _)) = expr.as_ref() {
                let num: f64 = n
                    .parse()
                    .map_err(|_| SqlError::translation(format!("Invalid number: {n}")))?;
                return Ok(Value::Number(-num));
            }
            Err(SqlError::translation(format!(
                "Expected numeric value, got: {expr:?}"
            )))
        }
        _ => Err(SqlError::translation(format!(
            "Expected value, got: {expr:?}"
        ))),
    }
}

fn parse_order_by(order_by: &[OrderByExpr]) -> Result<Option<OrderBy>, SqlError> {
    if order_by.is_empty() {
        return Ok(None);
    }

    if order_by.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple ORDER BY columns are not supported in Phase 1",
        ));
    }

    let order_expr = &order_by[0];
    let field = extract_identifier(&order_expr.expr)?;

    let direction = if order_expr.asc.unwrap_or(true) {
        SortDirection::Asc
    } else {
        SortDirection::Desc
    };

    Ok(Some(OrderBy { field, direction }))
}

fn parse_limit(limit: Option<&Expr>, offset: Option<&Offset>) -> Result<Option<Limit>, SqlError> {
    let count = match limit {
        Some(expr) => extract_limit_value(expr)?,
        None => return Ok(None),
    };

    let offset_value = match offset {
        Some(off) => {
            match &off.rows {
                OffsetRows::None | OffsetRows::Row | OffsetRows::Rows => {}
            }
            extract_limit_value(&off.value)?
        }
        None => 0,
    };

    Ok(Some(Limit {
        count,
        offset: offset_value,
    }))
}

fn extract_limit_value(expr: &Expr) -> Result<u64, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::Number(n, _)) => n
            .parse()
            .map_err(|_| SqlError::translation(format!("Invalid LIMIT value: {n}"))),
        _ => Err(SqlError::translation("LIMIT value must be a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(query.fields, vec!["name", "price", "category"]);
    }

    #[test]
    fn test_parse_select_with_alias() {
        let query = parse("SELECT name AS product_name FROM products").unwrap();
        assert_eq!(query.fields, vec!["name"]);
    }

    // WHERE clause tests
    #[test]
    fn test_parse_where_equals() {
        let query = parse("SELECT * FROM idx WHERE status = 'active'").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::Equals { field, value } => {
                assert_eq!(field, "status");
                assert!(matches!(value, Value::String(s) if s == "active"));
            }
            _ => panic!("Expected Equals condition"),
        }
    }

    #[test]
    fn test_parse_where_single_quoted_string() {
        let query = parse("SELECT * FROM idx WHERE name = 'test value'").unwrap();
        match &query.conditions[0] {
            Condition::Equals { value, .. } => {
                assert!(matches!(value, Value::String(s) if s == "test value"));
            }
            _ => panic!("Expected Equals condition"),
        }
    }

    #[test]
    fn test_parse_where_numeric() {
        let query = parse("SELECT * FROM idx WHERE price = 99.99").unwrap();
        match &query.conditions[0] {
            Condition::Equals { value, .. } => {
                assert!(matches!(value, Value::Number(n) if (*n - 99.99).abs() < f64::EPSILON));
            }
            _ => panic!("Expected Equals condition"),
        }
    }

    #[test]
    fn test_parse_where_negative_number() {
        let query = parse("SELECT * FROM idx WHERE temp = -10").unwrap();
        match &query.conditions[0] {
            Condition::Equals { value, .. } => {
                assert!(matches!(value, Value::Number(n) if (*n - (-10.0)).abs() < f64::EPSILON));
            }
            _ => panic!("Expected Equals condition"),
        }
    }

    #[test]
    fn test_parse_where_greater_than() {
        let query = parse("SELECT * FROM idx WHERE price > 100").unwrap();
        assert!(matches!(
            &query.conditions[0],
            Condition::GreaterThan { .. }
        ));
    }

    #[test]
    fn test_parse_where_greater_than_or_equal() {
        let query = parse("SELECT * FROM idx WHERE price >= 100").unwrap();
        assert!(matches!(
            &query.conditions[0],
            Condition::GreaterThanOrEqual { .. }
        ));
    }

    #[test]
    fn test_parse_where_less_than() {
        let query = parse("SELECT * FROM idx WHERE price < 100").unwrap();
        assert!(matches!(&query.conditions[0], Condition::LessThan { .. }));
    }

    #[test]
    fn test_parse_where_less_than_or_equal() {
        let query = parse("SELECT * FROM idx WHERE price <= 100").unwrap();
        assert!(matches!(
            &query.conditions[0],
            Condition::LessThanOrEqual { .. }
        ));
    }

    #[test]
    fn test_parse_where_between() {
        let query = parse("SELECT * FROM idx WHERE price BETWEEN 10 AND 100").unwrap();
        match &query.conditions[0] {
            Condition::Between { field, low, high } => {
                assert_eq!(field, "price");
                assert!(matches!(low, Value::Number(n) if *n == 10.0));
                assert!(matches!(high, Value::Number(n) if *n == 100.0));
            }
            _ => panic!("Expected Between condition"),
        }
    }

    #[test]
    fn test_parse_where_and() {
        let query = parse("SELECT * FROM idx WHERE a = 1 AND b = 2").unwrap();
        assert_eq!(query.conditions.len(), 2);
    }

    #[test]
    fn test_parse_where_nested() {
        let query = parse("SELECT * FROM idx WHERE (price > 100)").unwrap();
        assert!(matches!(
            &query.conditions[0],
            Condition::GreaterThan { .. }
        ));
    }

    #[test]
    fn test_parse_compound_identifier() {
        // Compound identifiers like table.column should use the last part
        let query = parse("SELECT idx.name FROM idx WHERE idx.price > 100").unwrap();
        assert_eq!(query.fields, vec!["name"]);
    }

    // ORDER BY tests
    #[test]
    fn test_parse_order_by_asc() {
        let query = parse("SELECT * FROM idx ORDER BY name ASC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.field, "name");
        assert_eq!(order_by.direction, SortDirection::Asc);
    }

    #[test]
    fn test_parse_order_by_desc() {
        let query = parse("SELECT * FROM idx ORDER BY price DESC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.field, "price");
        assert_eq!(order_by.direction, SortDirection::Desc);
    }

    #[test]
    fn test_parse_order_by_default_asc() {
        // Default should be ASC when not specified
        let query = parse("SELECT * FROM idx ORDER BY name").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.direction, SortDirection::Asc);
    }

    // LIMIT tests
    #[test]
    fn test_parse_limit() {
        let query = parse("SELECT * FROM idx LIMIT 50").unwrap();
        let limit = query.limit.unwrap();
        assert_eq!(limit.count, 50);
        assert_eq!(limit.offset, 0);
    }

    #[test]
    fn test_parse_limit_with_offset() {
        let query = parse("SELECT * FROM idx LIMIT 20 OFFSET 10").unwrap();
        let limit = query.limit.unwrap();
        assert_eq!(limit.count, 20);
        assert_eq!(limit.offset, 10);
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
    fn test_parse_multiple_order_by_not_supported() {
        let result = parse("SELECT * FROM idx ORDER BY a, b");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Multiple ORDER BY"));
    }

    #[test]
    fn test_parse_not_between() {
        let result = parse("SELECT * FROM idx WHERE x NOT BETWEEN 1 AND 10");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("NOT BETWEEN"));
    }

    #[test]
    fn test_parse_or_operator_not_supported() {
        let result = parse("SELECT * FROM idx WHERE a = 1 OR b = 2");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unsupported_expression() {
        // Subquery is not supported
        let result = parse("SELECT * FROM idx WHERE id IN (SELECT id FROM other)");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unsupported_select_item() {
        // Aggregate functions not supported
        let result = parse("SELECT COUNT(*) FROM idx");
        assert!(result.is_err());
    }

    // Case insensitivity tests
    #[test]
    fn test_parse_case_insensitive_keywords() {
        let query1 = parse("SELECT * FROM idx").unwrap();
        let query2 = parse("select * from idx").unwrap();
        let query3 = parse("SeLeCt * FrOm idx").unwrap();
        assert_eq!(query1.index_name, query2.index_name);
        assert_eq!(query2.index_name, query3.index_name);
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
    }

    // IN clause tests
    #[test]
    fn test_parse_in_string_values() {
        let query =
            parse("SELECT * FROM idx WHERE category IN ('electronics', 'accessories')").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::In {
                field,
                values,
                negated,
            } => {
                assert_eq!(field, "category");
                assert_eq!(values.len(), 2);
                assert!(!negated);
                assert!(matches!(&values[0], Value::String(s) if s == "electronics"));
                assert!(matches!(&values[1], Value::String(s) if s == "accessories"));
            }
            _ => panic!("Expected In condition"),
        }
    }

    #[test]
    fn test_parse_in_numeric_values() {
        let query = parse("SELECT * FROM idx WHERE price IN (10, 20, 30)").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::In {
                field,
                values,
                negated,
            } => {
                assert_eq!(field, "price");
                assert_eq!(values.len(), 3);
                assert!(!negated);
                assert!(matches!(&values[0], Value::Number(n) if *n == 10.0));
                assert!(matches!(&values[1], Value::Number(n) if *n == 20.0));
                assert!(matches!(&values[2], Value::Number(n) if *n == 30.0));
            }
            _ => panic!("Expected In condition"),
        }
    }

    #[test]
    fn test_parse_not_in() {
        let query = parse("SELECT * FROM idx WHERE status NOT IN ('deleted', 'archived')").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::In {
                field,
                values,
                negated,
            } => {
                assert_eq!(field, "status");
                assert_eq!(values.len(), 2);
                assert!(negated);
            }
            _ => panic!("Expected In condition"),
        }
    }

    #[test]
    fn test_parse_in_single_value() {
        let query = parse("SELECT * FROM idx WHERE type IN ('premium')").unwrap();
        match &query.conditions[0] {
            Condition::In { values, .. } => {
                assert_eq!(values.len(), 1);
            }
            _ => panic!("Expected In condition"),
        }
    }

    #[test]
    fn test_parse_in_with_and() {
        let query =
            parse("SELECT * FROM idx WHERE category IN ('a', 'b') AND status = 'active'").unwrap();
        assert_eq!(query.conditions.len(), 2);
        assert!(matches!(&query.conditions[0], Condition::In { .. }));
        assert!(matches!(&query.conditions[1], Condition::Equals { .. }));
    }
}
