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
#[derive(Debug, Clone, Default)]
pub struct SelectQuery {
    /// Fields to return. Empty means SELECT *.
    pub fields: Vec<SelectField>,
    /// The index name (from FROM clause).
    pub index_name: String,
    /// WHERE clause conditions.
    pub conditions: Vec<Condition>,
    /// ORDER BY clause.
    pub order_by: Option<OrderBy>,
    /// LIMIT clause.
    pub limit: Option<Limit>,
    /// Whether DISTINCT was specified.
    pub distinct: bool,
    /// Aggregate functions in SELECT clause.
    pub aggregates: Vec<AggregateExpr>,
    /// GROUP BY clause.
    pub group_by: Option<GroupBy>,
    /// HAVING clause (filter on aggregates).
    pub having: Option<Condition>,
    /// Vector KNN search (ORDER BY field <-> vector).
    pub vector_search: Option<VectorSearch>,
    /// Hybrid search configuration.
    pub hybrid_search: Option<HybridSearch>,
}

/// A field in the SELECT clause.
#[derive(Debug, Clone)]
pub struct SelectField {
    /// The field name or expression.
    pub name: String,
    /// Optional alias (AS clause).
    pub alias: Option<String>,
}

/// An aggregate function expression.
#[derive(Debug, Clone)]
pub struct AggregateExpr {
    /// The aggregate function type.
    pub function: AggregateFunction,
    /// The field to aggregate (None for COUNT(*)).
    pub field: Option<String>,
    /// Optional alias.
    pub alias: Option<String>,
}

/// Supported aggregate functions.
#[derive(Debug, Clone, PartialEq)]
pub enum AggregateFunction {
    /// COUNT(*) or COUNT(field)
    Count,
    /// SUM(field)
    Sum,
    /// AVG(field)
    Avg,
    /// MIN(field)
    Min,
    /// MAX(field)
    Max,
    /// COUNT_DISTINCT(field) - exact unique count
    CountDistinct,
    /// COUNT_DISTINCTISH(field) - approximate unique count using HyperLogLog
    CountDistinctish,
    /// STDDEV(field) - standard deviation
    Stddev,
    /// QUANTILE(field, percentile) - percentile value (0.0-1.0)
    Quantile { percentile: f64 },
    /// TOLIST(field) - collect all values into an array
    Tolist,
    /// FIRST_VALUE(field BY sort_field ASC|DESC) - first value when sorted
    FirstValue { sort_field: String, ascending: bool },
    /// RANDOM_SAMPLE(field, size) - random sample of values (max 1000)
    RandomSample { size: u32 },
    /// HLL(field) - returns raw HyperLogLog struct
    Hll,
    /// HLL_SUM(field) - merges HyperLogLog values
    HllSum,
}

impl std::fmt::Display for AggregateFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Count => write!(f, "COUNT"),
            Self::Sum => write!(f, "SUM"),
            Self::Avg => write!(f, "AVG"),
            Self::Min => write!(f, "MIN"),
            Self::Max => write!(f, "MAX"),
            Self::CountDistinct => write!(f, "COUNT_DISTINCT"),
            Self::CountDistinctish => write!(f, "COUNT_DISTINCTISH"),
            Self::Stddev => write!(f, "STDDEV"),
            Self::Quantile { .. } => write!(f, "QUANTILE"),
            Self::Tolist => write!(f, "TOLIST"),
            Self::FirstValue { .. } => write!(f, "FIRST_VALUE"),
            Self::RandomSample { .. } => write!(f, "RANDOM_SAMPLE"),
            Self::Hll => write!(f, "HLL"),
            Self::HllSum => write!(f, "HLL_SUM"),
        }
    }
}

/// GROUP BY clause.
#[derive(Debug, Clone)]
pub struct GroupBy {
    /// Fields to group by.
    pub fields: Vec<String>,
}

/// Distance metric for vector similarity search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistanceMetric {
    /// L2 (Euclidean) distance - operator `<->`
    #[default]
    L2,
    /// Cosine distance - operator `<=>`
    Cosine,
    /// Inner product - operator `<#>`
    InnerProduct,
}

impl std::fmt::Display for DistanceMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::L2 => write!(f, "L2"),
            Self::Cosine => write!(f, "COSINE"),
            Self::InnerProduct => write!(f, "IP"),
        }
    }
}

/// Vector search configuration for KNN queries.
#[derive(Debug, Clone)]
pub struct VectorSearch {
    /// The vector field name (e.g., "embedding").
    pub field: String,
    /// The vector blob (as string representation, e.g., "[0.1, 0.2, 0.3]").
    pub vector: String,
    /// Number of nearest neighbors to return (K).
    pub k: usize,
    /// Distance metric for the search (L2, Cosine, InnerProduct).
    /// Note: RediSearch uses the index's distance metric, not query-time.
    /// This field is used for documentation and potential validation.
    pub distance_metric: DistanceMetric,
}

/// Hybrid search configuration combining text and vector search.
#[derive(Debug, Clone)]
pub struct HybridSearch {
    /// Vector field configuration.
    pub vector: VectorSearch,
    /// Weight for vector scoring (0.0 to 1.0).
    pub vector_weight: f64,
    /// Weight for text scoring (0.0 to 1.0).
    pub text_weight: f64,
}

impl SelectQuery {
    /// Create a new SelectQuery with an index name.
    pub fn new(index_name: impl Into<String>) -> Self {
        Self {
            index_name: index_name.into(),
            ..Default::default()
        }
    }

    /// Add a field to the query.
    pub fn with_field(mut self, name: impl Into<String>) -> Self {
        self.fields.push(SelectField {
            name: name.into(),
            alias: None,
        });
        self
    }

    /// Add a field with alias to the query.
    pub fn with_field_alias(mut self, name: impl Into<String>, alias: impl Into<String>) -> Self {
        self.fields.push(SelectField {
            name: name.into(),
            alias: Some(alias.into()),
        });
        self
    }

    /// Add a condition to the query.
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.conditions.push(condition);
        self
    }
}

impl SelectField {
    /// Create a new SelectField.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
        }
    }

    /// Create a new SelectField with alias.
    pub fn with_alias(name: impl Into<String>, alias: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: Some(alias.into()),
        }
    }
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
    /// Not equals: field != value (or field <> value)
    NotEquals { field: String, value: Value },
    /// Like pattern: field LIKE 'pattern%'
    Like {
        field: String,
        pattern: String,
        negated: bool,
    },
    /// Is null: field IS NULL
    IsNull { field: String, negated: bool },
    /// AND expression: combines conditions with AND
    And(Box<Condition>, Box<Condition>),
    /// OR expression: combines conditions with OR
    Or(Box<Condition>, Box<Condition>),
    /// NOT expression: negates a condition
    Not(Box<Condition>),
}

impl Condition {
    /// Returns the field name this condition applies to.
    /// For OR conditions, returns the field from the left side.
    pub fn field(&self) -> &str {
        match self {
            Self::Equals { field, .. }
            | Self::GreaterThan { field, .. }
            | Self::GreaterThanOrEqual { field, .. }
            | Self::LessThan { field, .. }
            | Self::LessThanOrEqual { field, .. }
            | Self::Between { field, .. }
            | Self::In { field, .. }
            | Self::NotEquals { field, .. }
            | Self::Like { field, .. }
            | Self::IsNull { field, .. } => field,
            Self::Or(left, _) => left.field(),
            Self::And(left, _) => left.field(),
            Self::Not(inner) => inner.field(),
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

/// A single ORDER BY column with direction.
#[derive(Debug, Clone)]
pub struct OrderByColumn {
    /// Field to sort by.
    pub field: String,
    /// Sort direction.
    pub direction: SortDirection,
}

/// ORDER BY clause supporting multiple columns.
#[derive(Debug, Clone)]
pub struct OrderBy {
    /// Columns to sort by, in order of precedence.
    pub columns: Vec<OrderByColumn>,
}

impl OrderBy {
    /// Create a new OrderBy with a single column.
    pub fn single(field: impl Into<String>, direction: SortDirection) -> Self {
        Self {
            columns: vec![OrderByColumn {
                field: field.into(),
                direction,
            }],
        }
    }

    /// Get the first column (for backwards compatibility and simple cases).
    pub fn first(&self) -> Option<&OrderByColumn> {
        self.columns.first()
    }
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
        let val = Value::Number(12.34);
        assert_eq!(val.to_rql_string(), "12.34");
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
        let query = SelectQuery::new("products")
            .with_field("name")
            .with_field("price");
        assert_eq!(query.fields.len(), 2);
        assert_eq!(query.index_name, "products");
        assert!(query.conditions.is_empty());
    }

    #[test]
    fn test_select_query_with_order_and_limit() {
        let mut query = SelectQuery::new("products");
        query.order_by = Some(OrderBy::single("price", SortDirection::Desc));
        query.limit = Some(Limit {
            count: 10,
            offset: 5,
        });
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
        let ob = OrderBy::single("price", SortDirection::Asc);
        let cloned = ob.clone();
        let first = ob.first().unwrap();
        let cloned_first = cloned.first().unwrap();
        assert_eq!(first.field, cloned_first.field);
        assert_eq!(first.direction, cloned_first.direction);
    }

    #[test]
    fn test_order_by_multiple_columns() {
        let ob = OrderBy {
            columns: vec![
                OrderByColumn {
                    field: "category".to_string(),
                    direction: SortDirection::Asc,
                },
                OrderByColumn {
                    field: "price".to_string(),
                    direction: SortDirection::Desc,
                },
            ],
        };
        assert_eq!(ob.columns.len(), 2);
        assert_eq!(ob.columns[0].field, "category");
        assert_eq!(ob.columns[0].direction, SortDirection::Asc);
        assert_eq!(ob.columns[1].field, "price");
        assert_eq!(ob.columns[1].direction, SortDirection::Desc);
    }

    #[test]
    fn test_condition_not() {
        let inner = Condition::Equals {
            field: "category".to_string(),
            value: Value::String("electronics".to_string()),
        };
        let not_cond = Condition::Not(Box::new(inner));
        assert_eq!(not_cond.field(), "category");
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
        let query = SelectQuery::new("idx")
            .with_field("name")
            .with_condition(Condition::Equals {
                field: "x".to_string(),
                value: Value::Number(1.0),
            });
        let cloned = query.clone();
        assert_eq!(query.index_name, cloned.index_name);
    }
}
