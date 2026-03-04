/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! Internal AST representation for SQL queries.
//!
//! This module defines a simplified AST that captures only the SQL
//! constructs supported by the RediSearch query language.

/// A simplified SELECT query representation.
#[derive(Debug, Clone)]
pub struct SelectQuery {
    /// Fields to return. Empty means SELECT *.
    pub fields: Vec<String>,
    /// The index name (from FROM clause).
    pub index_name: String,
    /// WHERE clause conditions.
    pub conditions: Vec<Condition>,
    /// ORDER BY clause.
    pub order_by: Option<OrderBy>,
    /// LIMIT clause.
    pub limit: Option<Limit>,
}

/// A single condition in the WHERE clause.
#[derive(Debug, Clone)]
pub enum Condition {
    /// Equality: field = value
    Equals { field: String, value: Value },
    /// Greater than: field > value
    GreaterThan { field: String, value: Value },
    /// Greater than or equal: field >= value
    GreaterThanOrEqual { field: String, value: Value },
    /// Less than: field < value
    LessThan { field: String, value: Value },
    /// Less than or equal: field <= value
    LessThanOrEqual { field: String, value: Value },
    /// Between: field BETWEEN low AND high
    Between {
        field: String,
        low: Value,
        high: Value,
    },
    /// In: field IN (value1, value2, ...)
    In {
        field: String,
        values: Vec<Value>,
        negated: bool,
    },
}

impl Condition {
    /// Returns the field name this condition applies to.
    pub fn field(&self) -> &str {
        match self {
            Self::Equals { field, .. }
            | Self::GreaterThan { field, .. }
            | Self::GreaterThanOrEqual { field, .. }
            | Self::LessThan { field, .. }
            | Self::LessThanOrEqual { field, .. }
            | Self::Between { field, .. }
            | Self::In { field, .. } => field,
        }
    }
}

/// A value in a SQL expression.
#[derive(Debug, Clone)]
pub enum Value {
    /// A string literal.
    String(String),
    /// A numeric literal (integer or float).
    Number(f64),
}

impl Value {
    /// Converts the value to a string representation suitable for RQL.
    pub fn to_rql_string(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Number(n) => {
                // Format integers without decimal point
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
        }
    }
}

/// ORDER BY clause.
#[derive(Debug, Clone)]
pub struct OrderBy {
    /// Field to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction for ORDER BY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

impl std::fmt::Display for SortDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Asc => write!(f, "ASC"),
            Self::Desc => write!(f, "DESC"),
        }
    }
}

/// LIMIT clause.
#[derive(Debug, Clone, Copy)]
pub struct Limit {
    /// Maximum number of results to return.
    pub count: u64,
    /// Number of results to skip (OFFSET).
    pub offset: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Condition::field() tests
    #[test]
    fn test_condition_field_equals() {
        let cond = Condition::Equals {
            field: "status".to_string(),
            value: Value::String("active".to_string()),
        };
        assert_eq!(cond.field(), "status");
    }

    #[test]
    fn test_condition_field_greater_than() {
        let cond = Condition::GreaterThan {
            field: "price".to_string(),
            value: Value::Number(100.0),
        };
        assert_eq!(cond.field(), "price");
    }

    #[test]
    fn test_condition_field_greater_than_or_equal() {
        let cond = Condition::GreaterThanOrEqual {
            field: "count".to_string(),
            value: Value::Number(5.0),
        };
        assert_eq!(cond.field(), "count");
    }

    #[test]
    fn test_condition_field_less_than() {
        let cond = Condition::LessThan {
            field: "age".to_string(),
            value: Value::Number(18.0),
        };
        assert_eq!(cond.field(), "age");
    }

    #[test]
    fn test_condition_field_less_than_or_equal() {
        let cond = Condition::LessThanOrEqual {
            field: "rating".to_string(),
            value: Value::Number(5.0),
        };
        assert_eq!(cond.field(), "rating");
    }

    #[test]
    fn test_condition_field_between() {
        let cond = Condition::Between {
            field: "price".to_string(),
            low: Value::Number(10.0),
            high: Value::Number(100.0),
        };
        assert_eq!(cond.field(), "price");
    }

    #[test]
    fn test_condition_field_in() {
        let cond = Condition::In {
            field: "category".to_string(),
            values: vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
            ],
            negated: false,
        };
        assert_eq!(cond.field(), "category");
    }

    // Value::to_rql_string() tests
    #[test]
    fn test_value_to_rql_string_string() {
        let val = Value::String("hello".to_string());
        assert_eq!(val.to_rql_string(), "hello");
    }

    #[test]
    fn test_value_to_rql_string_integer() {
        let val = Value::Number(42.0);
        assert_eq!(val.to_rql_string(), "42");
    }

    #[test]
    fn test_value_to_rql_string_float() {
        let val = Value::Number(3.14);
        assert_eq!(val.to_rql_string(), "3.14");
    }

    #[test]
    fn test_value_to_rql_string_negative_integer() {
        let val = Value::Number(-10.0);
        assert_eq!(val.to_rql_string(), "-10");
    }

    #[test]
    fn test_value_to_rql_string_negative_float() {
        let val = Value::Number(-2.5);
        assert_eq!(val.to_rql_string(), "-2.5");
    }

    #[test]
    fn test_value_to_rql_string_zero() {
        let val = Value::Number(0.0);
        assert_eq!(val.to_rql_string(), "0");
    }

    #[test]
    fn test_value_to_rql_string_large_integer() {
        let val = Value::Number(1_000_000.0);
        assert_eq!(val.to_rql_string(), "1000000");
    }

    // SortDirection Display tests
    #[test]
    fn test_sort_direction_display_asc() {
        assert_eq!(format!("{}", SortDirection::Asc), "ASC");
    }

    #[test]
    fn test_sort_direction_display_desc() {
        assert_eq!(format!("{}", SortDirection::Desc), "DESC");
    }

    // SelectQuery tests
    #[test]
    fn test_select_query_new() {
        let query = SelectQuery {
            fields: vec!["name".to_string(), "price".to_string()],
            index_name: "products".to_string(),
            conditions: vec![],
            order_by: Some(OrderBy {
                field: "price".to_string(),
                direction: SortDirection::Desc,
            }),
            limit: Some(Limit {
                count: 10,
                offset: 5,
            }),
        };
        assert_eq!(query.fields.len(), 2);
        assert_eq!(query.index_name, "products");
        assert!(query.conditions.is_empty());
        assert!(query.order_by.is_some());
        assert!(query.limit.is_some());
    }

    // Clone tests
    #[test]
    fn test_condition_clone() {
        let cond = Condition::Equals {
            field: "test".to_string(),
            value: Value::Number(1.0),
        };
        let cloned = cond.clone();
        assert_eq!(cond.field(), cloned.field());
    }

    #[test]
    fn test_value_clone() {
        let val = Value::String("test".to_string());
        let cloned = val.clone();
        assert_eq!(val.to_rql_string(), cloned.to_rql_string());
    }

    #[test]
    fn test_order_by_clone() {
        let ob = OrderBy {
            field: "price".to_string(),
            direction: SortDirection::Asc,
        };
        let cloned = ob.clone();
        assert_eq!(ob.field, cloned.field);
        assert_eq!(ob.direction, cloned.direction);
    }

    #[test]
    fn test_limit_clone() {
        let limit = Limit {
            count: 10,
            offset: 5,
        };
        let cloned = limit;
        assert_eq!(limit.count, cloned.count);
        assert_eq!(limit.offset, cloned.offset);
    }

    #[test]
    fn test_select_query_clone() {
        let query = SelectQuery {
            fields: vec!["name".to_string()],
            index_name: "idx".to_string(),
            conditions: vec![Condition::Equals {
                field: "x".to_string(),
                value: Value::Number(1.0),
            }],
            order_by: None,
            limit: None,
        };
        let cloned = query.clone();
        assert_eq!(query.index_name, cloned.index_name);
    }
}
