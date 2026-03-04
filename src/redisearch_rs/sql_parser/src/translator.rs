/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL AST → RQL conversion.
//!
//! This module translates our internal SQL AST representation into
//! RediSearch Query Language (RQL) strings and arguments.

use crate::ast::{Condition, SelectQuery, Value};
use crate::error::SqlError;
use crate::{Command, Translation};

/// Translates a parsed SQL query into a RQL translation.
pub fn translate(query: SelectQuery) -> Result<Translation, SqlError> {
    let query_string = build_query_string(&query.conditions)?;
    let arguments = build_arguments(&query)?;

    Ok(Translation {
        command: Command::Search,
        index_name: query.index_name,
        query_string,
        arguments,
    })
}

/// Builds the RQL query string from WHERE clause conditions.
fn build_query_string(conditions: &[Condition]) -> Result<String, SqlError> {
    if conditions.is_empty() {
        return Ok("*".to_string());
    }

    let parts: Vec<String> = conditions
        .iter()
        .map(translate_condition)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(parts.join(" "))
}

/// Translates a single condition to RQL syntax.
fn translate_condition(condition: &Condition) -> Result<String, SqlError> {
    match condition {
        Condition::Equals { field, value } => match value {
            // String equality uses TAG syntax for exact match
            Value::String(s) => Ok(format!("@{field}:{{{s}}}")),
            // Numeric equality uses range syntax [n n]
            Value::Number(_) => {
                let n_str = value.to_rql_string();
                Ok(format!("@{field}:[{n_str} {n_str}]"))
            }
        },
        Condition::GreaterThan { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}:[({val_str} +inf]"))
        }
        Condition::GreaterThanOrEqual { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}:[{val_str} +inf]"))
        }
        Condition::LessThan { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}:[-inf ({val_str}]"))
        }
        Condition::LessThanOrEqual { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}:[-inf {val_str}]"))
        }
        Condition::Between { field, low, high } => {
            let low_str = low.to_rql_string();
            let high_str = high.to_rql_string();
            Ok(format!("@{field}:[{low_str} {high_str}]"))
        }
    }
}

/// Builds additional RQL arguments (RETURN, SORTBY, LIMIT).
fn build_arguments(query: &SelectQuery) -> Result<Vec<String>, SqlError> {
    let mut args = Vec::new();

    // RETURN clause
    if !query.fields.is_empty() {
        args.push("RETURN".to_string());
        args.push(query.fields.len().to_string());
        args.extend(query.fields.iter().cloned());
    }

    // SORTBY clause
    if let Some(order_by) = &query.order_by {
        args.push("SORTBY".to_string());
        args.push(order_by.field.clone());
        args.push(order_by.direction.to_string());
    }

    // LIMIT clause
    if let Some(limit) = &query.limit {
        args.push("LIMIT".to_string());
        args.push(limit.offset.to_string());
        args.push(limit.count.to_string());
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value;

    #[test]
    fn test_translate_empty_conditions() {
        let query = SelectQuery {
            fields: vec![],
            index_name: "idx".to_string(),
            conditions: vec![],
            order_by: None,
            limit: None,
        };
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "*");
    }

    #[test]
    fn test_translate_string_equality() {
        let query = SelectQuery {
            fields: vec![],
            index_name: "idx".to_string(),
            conditions: vec![Condition::Equals {
                field: "status".to_string(),
                value: Value::String("active".to_string()),
            }],
            order_by: None,
            limit: None,
        };
        let result = translate(query).unwrap();
        // String equality uses TAG syntax with curly braces
        assert_eq!(result.query_string, "@status:{active}");
    }

    #[test]
    fn test_translate_numeric_equality() {
        let query = SelectQuery {
            fields: vec![],
            index_name: "idx".to_string(),
            conditions: vec![Condition::Equals {
                field: "count".to_string(),
                value: Value::Number(42.0),
            }],
            order_by: None,
            limit: None,
        };
        let result = translate(query).unwrap();
        // Numeric equality uses range syntax [n n]
        assert_eq!(result.query_string, "@count:[42 42]");
    }

    #[test]
    fn test_translate_range() {
        let query = SelectQuery {
            fields: vec![],
            index_name: "idx".to_string(),
            conditions: vec![Condition::GreaterThan {
                field: "price".to_string(),
                value: Value::Number(100.0),
            }],
            order_by: None,
            limit: None,
        };
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@price:[(100 +inf]");
    }

    #[test]
    fn test_translate_with_return() {
        let query = SelectQuery {
            fields: vec!["name".to_string(), "price".to_string()],
            index_name: "products".to_string(),
            conditions: vec![],
            order_by: None,
            limit: None,
        };
        let result = translate(query).unwrap();
        assert!(result.arguments.contains(&"RETURN".to_string()));
        assert!(result.arguments.contains(&"2".to_string()));
    }
}

