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

use crate::ast::{AggregateExpr, Condition, SelectQuery, Value};
use crate::error::SqlError;
use crate::{Command, Translation};

/// Translates a parsed SQL query into a RQL translation.
pub fn translate(query: SelectQuery) -> Result<Translation, SqlError> {
    // Determine command type based on query features
    let command = determine_command(&query);

    let query_string = build_query_string(&query)?;
    let arguments = build_arguments(&query, command)?;

    Ok(Translation {
        command,
        index_name: query.index_name,
        query_string,
        arguments,
    })
}

/// Determine which Redis command to use.
#[expect(clippy::missing_const_for_fn)]
fn determine_command(query: &SelectQuery) -> Command {
    // Use FT.HYBRID for weighted hybrid search
    if query.hybrid_search.is_some() {
        Command::Hybrid
    }
    // Use FT.AGGREGATE for GROUP BY, DISTINCT, or aggregate functions
    else if query.group_by.is_some() || query.distinct || !query.aggregates.is_empty() {
        Command::Aggregate
    } else {
        Command::Search
    }
}

/// Builds the RQL query string from WHERE clause conditions.
fn build_query_string(query: &SelectQuery) -> Result<String, SqlError> {
    // For FT.HYBRID, the query string is just the text query (no KNN syntax in query)
    if query.hybrid_search.is_some() {
        return build_hybrid_query_string(query);
    }

    let base_query = if query.conditions.is_empty() {
        "*".to_string()
    } else {
        let parts: Vec<String> = query
            .conditions
            .iter()
            .map(translate_condition)
            .collect::<Result<Vec<_>, _>>()?;
        parts.join(" ")
    };

    // For vector search, append KNN operator
    if let Some(ref vs) = query.vector_search {
        // Format: "base_query=>[KNN K @field $BLOB]"
        Ok(format!(
            "{}=>[KNN {} @{} $BLOB]",
            base_query, vs.k, vs.field
        ))
    } else {
        Ok(base_query)
    }
}

/// Builds the query string for FT.HYBRID (just the text query part).
fn build_hybrid_query_string(query: &SelectQuery) -> Result<String, SqlError> {
    // For FT.HYBRID, we only include the text filter part
    // The vector search is handled separately in arguments
    if query.conditions.is_empty() {
        Ok("*".to_string())
    } else {
        let parts: Vec<String> = query
            .conditions
            .iter()
            .map(translate_condition)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(parts.join(" "))
    }
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
        Condition::NotEquals { field, value } => match value {
            // String not-equals uses negated TAG syntax
            Value::String(s) => Ok(format!("-@{field}:{{{s}}}")),
            // Numeric not-equals uses negated range syntax
            Value::Number(_) => {
                let n_str = value.to_rql_string();
                Ok(format!("-@{field}:[{n_str} {n_str}]"))
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
        Condition::In {
            field,
            values,
            negated,
        } => {
            // Check if values are strings or numbers
            let all_strings = values.iter().all(|v| matches!(v, Value::String(_)));

            let prefix = if *negated { "-" } else { "" };

            if all_strings {
                // TAG syntax for strings: @field:{val1|val2|val3}
                let values_str = values
                    .iter()
                    .map(|v| v.to_rql_string())
                    .collect::<Vec<_>>()
                    .join("|");
                Ok(format!("{prefix}@{field}:{{{values_str}}}"))
            } else {
                // Numeric IN: use OR of exact matches
                // @field:[n1 n1] | @field:[n2 n2] | ...
                let parts: Vec<String> = values
                    .iter()
                    .map(|v| {
                        let n_str = v.to_rql_string();
                        format!("@{field}:[{n_str} {n_str}]")
                    })
                    .collect();

                if *negated {
                    // For NOT IN with numbers, negate the whole expression
                    Ok(format!("-({})", parts.join("|")))
                } else {
                    Ok(format!("({})", parts.join("|")))
                }
            }
        }
        Condition::Like {
            field,
            pattern,
            negated,
        } => {
            // Convert SQL LIKE pattern to RQL wildcard
            // SQL % -> RQL *
            // SQL _ -> RQL ?
            let rql_pattern = pattern.replace('%', "*").replace('_', "?");
            let prefix = if *negated { "-" } else { "" };
            Ok(format!("{prefix}@{field}:{rql_pattern}"))
        }
        Condition::IsNull { field, negated } => {
            // RediSearch uses ismissing() for null checks
            if *negated {
                // IS NOT NULL: field exists and has a value
                Ok(format!("-ismissing(@{field})"))
            } else {
                // IS NULL: field is missing
                Ok(format!("ismissing(@{field})"))
            }
        }
        Condition::Or(left, right) => {
            // OR conditions in RQL use | operator with spaces between parenthesized sub-conditions
            let left_str = translate_condition(left)?;
            let right_str = translate_condition(right)?;
            Ok(format!("({left_str}) | ({right_str})"))
        }
        Condition::Not(inner) => {
            // NOT negates the inner condition with - prefix
            let inner_str = translate_condition(inner)?;
            // Wrap in parentheses and negate
            Ok(format!("-({inner_str})"))
        }
    }
}

/// Builds additional RQL arguments (RETURN, SORTBY, LIMIT, GROUPBY, REDUCE, etc.).
fn build_arguments(query: &SelectQuery, command: Command) -> Result<Vec<String>, SqlError> {
    let mut args = Vec::new();

    match command {
        Command::Search => {
            // RETURN clause for FT.SEARCH
            if !query.fields.is_empty() {
                args.push("RETURN".to_string());
                // Count fields including aliases
                let field_count: usize = query
                    .fields
                    .iter()
                    .map(|f| if f.alias.is_some() { 3 } else { 1 }) // name [AS alias]
                    .sum();
                args.push(field_count.to_string());
                for field in &query.fields {
                    args.push(field.name.clone());
                    if let Some(alias) = &field.alias {
                        args.push("AS".to_string());
                        args.push(alias.clone());
                    }
                }
            }

            // SORTBY clause (only if not using vector search)
            if query.vector_search.is_none()
                && let Some(order_by) = &query.order_by
            {
                args.push("SORTBY".to_string());
                // Add all columns: field1 ASC field2 DESC ...
                for col in &order_by.columns {
                    args.push(col.field.clone());
                    args.push(col.direction.to_string());
                }
            }

            // LIMIT clause (for non-vector queries)
            if query.vector_search.is_none()
                && let Some(limit) = &query.limit
            {
                args.push("LIMIT".to_string());
                args.push(limit.offset.to_string());
                args.push(limit.count.to_string());
            }

            // PARAMS for vector search
            if let Some(ref vs) = query.vector_search {
                args.push("PARAMS".to_string());
                args.push("2".to_string());
                args.push("BLOB".to_string());
                args.push(vs.vector.clone());
            }
        }
        Command::Hybrid => {
            // FT.HYBRID specific arguments
            if let Some(ref hs) = query.hybrid_search {
                // Vector configuration
                args.push("VECTOR".to_string());
                args.push(hs.vector.field.clone());
                args.push("K".to_string());
                args.push(hs.vector.k.to_string());
                args.push("VECTOR_BLOB".to_string());
                args.push(hs.vector.vector.clone());

                // Weights
                args.push("WEIGHT".to_string());
                args.push(hs.vector_weight.to_string());
                args.push("TEXT".to_string());
                args.push(hs.text_weight.to_string());
            }

            // LIMIT clause
            if let Some(limit) = &query.limit {
                args.push("LIMIT".to_string());
                args.push(limit.offset.to_string());
                args.push(limit.count.to_string());
            }
        }
        Command::Aggregate => {
            // Handle GROUPBY
            if let Some(group_by) = &query.group_by {
                args.push("GROUPBY".to_string());
                args.push(group_by.fields.len().to_string());
                for field in &group_by.fields {
                    args.push(format!("@{field}"));
                }

                // Add REDUCE clauses for aggregates
                for agg in &query.aggregates {
                    args.extend(build_reduce_clause(agg));
                }
            } else if query.distinct {
                // DISTINCT translates to GROUPBY on the selected fields
                if !query.fields.is_empty() {
                    args.push("GROUPBY".to_string());
                    args.push(query.fields.len().to_string());
                    for field in &query.fields {
                        args.push(format!("@{}", field.name));
                    }
                }
            } else if !query.aggregates.is_empty() {
                // Aggregates without GROUP BY - group by nothing
                args.push("GROUPBY".to_string());
                args.push("0".to_string());

                for agg in &query.aggregates {
                    args.extend(build_reduce_clause(agg));
                }
            }

            // HAVING translates to FILTER
            if let Some(having) = &query.having {
                let filter_expr = translate_having_condition(having)?;
                args.push("FILTER".to_string());
                args.push(filter_expr);
            }

            // SORTBY clause
            if let Some(order_by) = &query.order_by {
                args.push("SORTBY".to_string());
                args.push("1".to_string()); // Number of sort fields
                args.push(format!("@{}", order_by.field));
                args.push(order_by.direction.to_string());
            }

            // LIMIT clause
            if let Some(limit) = &query.limit {
                args.push("LIMIT".to_string());
                args.push(limit.offset.to_string());
                args.push(limit.count.to_string());
            }
        }
    }

    Ok(args)
}

/// Build a REDUCE clause for an aggregate function.
fn build_reduce_clause(agg: &AggregateExpr) -> Vec<String> {
    let mut args = Vec::new();
    args.push("REDUCE".to_string());
    args.push(agg.function.to_string());

    match &agg.field {
        Some(field) => {
            args.push("1".to_string()); // Number of arguments
            args.push(format!("@{field}"));
        }
        None => {
            args.push("0".to_string()); // COUNT(*) has 0 arguments
        }
    }

    // Add alias
    if let Some(alias) = &agg.alias {
        args.push("AS".to_string());
        args.push(alias.clone());
    } else {
        // Generate default alias
        let default_alias = match agg.field {
            Some(ref f) => format!("{}_{}", agg.function.to_string().to_lowercase(), f),
            None => agg.function.to_string().to_lowercase(),
        };
        args.push("AS".to_string());
        args.push(default_alias);
    }

    args
}

/// Translate HAVING condition to FT.AGGREGATE FILTER expression.
fn translate_having_condition(condition: &Condition) -> Result<String, SqlError> {
    // HAVING conditions in FT.AGGREGATE use @field syntax and comparison operators
    match condition {
        Condition::Equals { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}=={val_str}"))
        }
        Condition::NotEquals { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}!={val_str}"))
        }
        Condition::GreaterThan { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}>{val_str}"))
        }
        Condition::GreaterThanOrEqual { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}>={val_str}"))
        }
        Condition::LessThan { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}<{val_str}"))
        }
        Condition::LessThanOrEqual { field, value } => {
            let val_str = value.to_rql_string();
            Ok(format!("@{field}<={val_str}"))
        }
        _ => Err(SqlError::unsupported(
            "Complex HAVING conditions are not supported",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value;

    #[test]
    fn test_translate_empty_conditions() {
        let query = SelectQuery::new("idx");
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "*");
    }

    #[test]
    fn test_translate_string_equality() {
        let query = SelectQuery::new("idx").with_condition(Condition::Equals {
            field: "status".to_string(),
            value: Value::String("active".to_string()),
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@status:{active}");
    }

    #[test]
    fn test_translate_numeric_equality() {
        let query = SelectQuery::new("idx").with_condition(Condition::Equals {
            field: "count".to_string(),
            value: Value::Number(42.0),
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@count:[42 42]");
    }

    #[test]
    fn test_translate_range() {
        let query = SelectQuery::new("idx").with_condition(Condition::GreaterThan {
            field: "price".to_string(),
            value: Value::Number(100.0),
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@price:[(100 +inf]");
    }

    #[test]
    fn test_translate_with_return() {
        let query = SelectQuery::new("products")
            .with_field("name")
            .with_field("price");
        let result = translate(query).unwrap();
        assert!(result.arguments.contains(&"RETURN".to_string()));
    }

    #[test]
    fn test_translate_in_strings() {
        let query = SelectQuery::new("idx").with_condition(Condition::In {
            field: "category".to_string(),
            values: vec![
                Value::String("electronics".to_string()),
                Value::String("accessories".to_string()),
            ],
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@category:{electronics|accessories}");
    }

    #[test]
    fn test_translate_not_in_strings() {
        let query = SelectQuery::new("idx").with_condition(Condition::In {
            field: "status".to_string(),
            values: vec![
                Value::String("deleted".to_string()),
                Value::String("archived".to_string()),
            ],
            negated: true,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-@status:{deleted|archived}");
    }

    #[test]
    fn test_translate_in_numbers() {
        let query = SelectQuery::new("idx").with_condition(Condition::In {
            field: "price".to_string(),
            values: vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0),
            ],
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(
            result.query_string,
            "(@price:[10 10]|@price:[20 20]|@price:[30 30])"
        );
    }

    #[test]
    fn test_translate_not_in_numbers() {
        let query = SelectQuery::new("idx").with_condition(Condition::In {
            field: "count".to_string(),
            values: vec![Value::Number(1.0), Value::Number(2.0)],
            negated: true,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-(@count:[1 1]|@count:[2 2])");
    }

    #[test]
    fn test_translate_in_single_value() {
        let query = SelectQuery::new("idx").with_condition(Condition::In {
            field: "type".to_string(),
            values: vec![Value::String("premium".to_string())],
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@type:{premium}");
    }

    // New feature tests
    #[test]
    fn test_translate_not_equals_string() {
        let query = SelectQuery::new("idx").with_condition(Condition::NotEquals {
            field: "status".to_string(),
            value: Value::String("deleted".to_string()),
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-@status:{deleted}");
    }

    #[test]
    fn test_translate_not_equals_number() {
        let query = SelectQuery::new("idx").with_condition(Condition::NotEquals {
            field: "price".to_string(),
            value: Value::Number(100.0),
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-@price:[100 100]");
    }

    #[test]
    fn test_translate_like_prefix() {
        let query = SelectQuery::new("idx").with_condition(Condition::Like {
            field: "name".to_string(),
            pattern: "Lap%".to_string(),
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@name:Lap*");
    }

    #[test]
    fn test_translate_like_suffix() {
        let query = SelectQuery::new("idx").with_condition(Condition::Like {
            field: "name".to_string(),
            pattern: "%top".to_string(),
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@name:*top");
    }

    #[test]
    fn test_translate_like_contains() {
        let query = SelectQuery::new("idx").with_condition(Condition::Like {
            field: "name".to_string(),
            pattern: "%apt%".to_string(),
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "@name:*apt*");
    }

    #[test]
    fn test_translate_is_null() {
        let query = SelectQuery::new("idx").with_condition(Condition::IsNull {
            field: "category".to_string(),
            negated: false,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "ismissing(@category)");
    }

    #[test]
    fn test_translate_is_not_null() {
        let query = SelectQuery::new("idx").with_condition(Condition::IsNull {
            field: "category".to_string(),
            negated: true,
        });
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-ismissing(@category)");
    }

    #[test]
    fn test_translate_or() {
        let query = SelectQuery::new("idx").with_condition(Condition::Or(
            Box::new(Condition::Equals {
                field: "a".to_string(),
                value: Value::Number(1.0),
            }),
            Box::new(Condition::Equals {
                field: "b".to_string(),
                value: Value::Number(2.0),
            }),
        ));
        let result = translate(query).unwrap();
        // RQL OR requires spaces around pipe between parenthesized sub-conditions
        assert_eq!(result.query_string, "(@a:[1 1]) | (@b:[2 2])");
    }

    // Vector search tests
    #[test]
    fn test_translate_vector_knn_pure() {
        use crate::ast::{DistanceMetric, VectorSearch};
        let mut query = SelectQuery::new("products");
        query.vector_search = Some(VectorSearch {
            field: "embedding".to_string(),
            vector: "[0.1, 0.2, 0.3]".to_string(),
            k: 10,
            distance_metric: DistanceMetric::default(),
        });
        let result = translate(query).unwrap();
        // KNN syntax: "*=>[KNN K @field $BLOB]"
        assert_eq!(result.query_string, "*=>[KNN 10 @embedding $BLOB]");
        // PARAMS should contain the vector
        assert!(result.arguments.contains(&"PARAMS".to_string()));
        assert!(result.arguments.contains(&"BLOB".to_string()));
        assert!(result.arguments.contains(&"[0.1, 0.2, 0.3]".to_string()));
    }

    #[test]
    fn test_translate_vector_knn_with_filter() {
        use crate::ast::{DistanceMetric, VectorSearch};
        let mut query = SelectQuery::new("products");
        query.conditions.push(Condition::Equals {
            field: "category".to_string(),
            value: Value::String("electronics".to_string()),
        });
        query.vector_search = Some(VectorSearch {
            field: "embedding".to_string(),
            vector: "[0.1, 0.2]".to_string(),
            k: 5,
            distance_metric: DistanceMetric::default(),
        });
        let result = translate(query).unwrap();
        // Filter + KNN syntax: "filter=>[KNN K @field $BLOB]"
        assert_eq!(
            result.query_string,
            "@category:{electronics}=>[KNN 5 @embedding $BLOB]"
        );
    }

    #[test]
    fn test_translate_vector_knn_multiple_filters() {
        use crate::ast::{DistanceMetric, VectorSearch};
        let mut query = SelectQuery::new("products");
        query.conditions.push(Condition::Equals {
            field: "category".to_string(),
            value: Value::String("electronics".to_string()),
        });
        query.conditions.push(Condition::GreaterThan {
            field: "price".to_string(),
            value: Value::Number(100.0),
        });
        query.vector_search = Some(VectorSearch {
            field: "embedding".to_string(),
            vector: "[0.1]".to_string(),
            k: 3,
            distance_metric: DistanceMetric::default(),
        });
        let result = translate(query).unwrap();
        // Multiple filters combined: "@f1 @f2=>[KNN K @field $BLOB]"
        assert_eq!(
            result.query_string,
            "@category:{electronics} @price:[(100 +inf]=>[KNN 3 @embedding $BLOB]"
        );
    }
}
