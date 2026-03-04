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
        Condition::And(left, right) => {
            // AND conditions in RQL are implicit (space-separated) with parenthesized sub-conditions
            let left_str = translate_condition(left)?;
            let right_str = translate_condition(right)?;
            Ok(format!("({left_str}) ({right_str})"))
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
            // FT.SEARCH only supports single-column SORTBY
            if query.vector_search.is_none()
                && let Some(order_by) = &query.order_by
            {
                if order_by.columns.len() > 1 {
                    return Err(SqlError::unsupported(
                        "Multiple ORDER BY columns are not supported by FT.SEARCH",
                    )
                    .with_suggestion(
                        "Use a single ORDER BY column, or use GROUP BY to generate FT.AGGREGATE which supports multiple sort columns",
                    ));
                }
                args.push("SORTBY".to_string());
                // Add single column: field ASC/DESC
                if let Some(col) = order_by.columns.first() {
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
                let filter_expr = translate_having_condition(having, &query.aggregates)?;
                args.push("FILTER".to_string());
                args.push(filter_expr);
            }

            // SORTBY clause (FT.AGGREGATE format: SORTBY nargs @field1 ASC @field2 DESC ...)
            if let Some(order_by) = &query.order_by {
                args.push("SORTBY".to_string());
                // nargs = 2 per column (field + direction)
                args.push((order_by.columns.len() * 2).to_string());
                for col in &order_by.columns {
                    args.push(format!("@{}", col.field));
                    args.push(col.direction.to_string());
                }
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
    use crate::ast::AggregateFunction;

    let mut args = Vec::new();
    args.push("REDUCE".to_string());
    args.push(agg.function.to_string());

    // Build arguments based on function type
    match &agg.function {
        // Simple functions with 0 or 1 field argument
        AggregateFunction::Count
        | AggregateFunction::Sum
        | AggregateFunction::Avg
        | AggregateFunction::Min
        | AggregateFunction::Max
        | AggregateFunction::CountDistinct
        | AggregateFunction::CountDistinctish
        | AggregateFunction::Stddev
        | AggregateFunction::Tolist
        | AggregateFunction::Hll
        | AggregateFunction::HllSum => {
            match &agg.field {
                Some(field) => {
                    args.push("1".to_string());
                    args.push(format!("@{field}"));
                }
                None => {
                    args.push("0".to_string()); // COUNT(*) has 0 arguments
                }
            }
        }
        // QUANTILE(field, percentile) -> REDUCE QUANTILE 2 @field percentile
        AggregateFunction::Quantile { percentile } => {
            if let Some(field) = &agg.field {
                args.push("2".to_string());
                args.push(format!("@{field}"));
                args.push(percentile.to_string());
            }
        }
        // RANDOM_SAMPLE(field, size) -> REDUCE RANDOM_SAMPLE 2 @field size
        AggregateFunction::RandomSample { size } => {
            if let Some(field) = &agg.field {
                args.push("2".to_string());
                args.push(format!("@{field}"));
                args.push(size.to_string());
            }
        }
        // FIRST_VALUE(field BY sort_field ASC/DESC) -> REDUCE FIRST_VALUE 4 @field BY @sort_field ASC/DESC
        AggregateFunction::FirstValue {
            sort_field,
            ascending,
        } => {
            if let Some(field) = &agg.field {
                args.push("4".to_string());
                args.push(format!("@{field}"));
                args.push("BY".to_string());
                args.push(format!("@{sort_field}"));
                args.push(if *ascending {
                    "ASC".to_string()
                } else {
                    "DESC".to_string()
                });
            }
        }
    }

    // Add alias
    if let Some(alias) = &agg.alias {
        args.push("AS".to_string());
        args.push(alias.clone());
    } else {
        // Generate default alias based on function type
        let default_alias = generate_default_alias(agg);
        args.push("AS".to_string());
        args.push(default_alias);
    }

    args
}

/// Generate default alias for an aggregate function.
fn generate_default_alias(agg: &AggregateExpr) -> String {
    use crate::ast::AggregateFunction;

    let base_name = match &agg.function {
        AggregateFunction::Count => "count",
        AggregateFunction::Sum => "sum",
        AggregateFunction::Avg => "avg",
        AggregateFunction::Min => "min",
        AggregateFunction::Max => "max",
        AggregateFunction::CountDistinct => "count_distinct",
        AggregateFunction::CountDistinctish => "count_distinctish",
        AggregateFunction::Stddev => "stddev",
        AggregateFunction::Quantile { percentile } => {
            // Include percentile in alias for clarity
            return match &agg.field {
                Some(f) => format!("quantile_{}_{}", (percentile * 100.0) as u32, f),
                None => format!("quantile_{}", (percentile * 100.0) as u32),
            };
        }
        AggregateFunction::Tolist => "tolist",
        AggregateFunction::FirstValue { .. } => "first_value",
        AggregateFunction::RandomSample { size } => {
            return match &agg.field {
                Some(f) => format!("random_sample_{}_{}", size, f),
                None => format!("random_sample_{}", size),
            };
        }
        AggregateFunction::Hll => "hll",
        AggregateFunction::HllSum => "hll_sum",
    };

    match &agg.field {
        Some(f) => format!("{base_name}_{f}"),
        None => base_name.to_string(),
    }
}

/// Translate HAVING condition to FT.AGGREGATE FILTER expression.
///
/// The `aggregates` parameter is used to resolve the field name from the HAVING
/// condition to the actual alias used in REDUCE clauses. For example, if SELECT has
/// `COUNT(*) AS cnt` and HAVING has `COUNT(*) > 3`, the parser generates a condition
/// with field `"count"`, but we need to reference `@cnt` in the FILTER.
fn translate_having_condition(
    condition: &Condition,
    aggregates: &[AggregateExpr],
) -> Result<String, SqlError> {
    // HAVING conditions in FT.AGGREGATE use @field syntax and comparison operators
    match condition {
        Condition::Equals { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}=={val_str}"))
        }
        Condition::NotEquals { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}!={val_str}"))
        }
        Condition::GreaterThan { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}>{val_str}"))
        }
        Condition::GreaterThanOrEqual { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}>={val_str}"))
        }
        Condition::LessThan { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}<{val_str}"))
        }
        Condition::LessThanOrEqual { field, value } => {
            let resolved_field = resolve_having_field(field, aggregates);
            let val_str = value.to_rql_string();
            Ok(format!("@{resolved_field}<={val_str}"))
        }
        Condition::And(left, right) => {
            // AND in FILTER expressions uses &&
            let left_str = translate_having_condition(left, aggregates)?;
            let right_str = translate_having_condition(right, aggregates)?;
            Ok(format!("({left_str} && {right_str})"))
        }
        Condition::Or(left, right) => {
            // OR in FILTER expressions uses ||
            let left_str = translate_having_condition(left, aggregates)?;
            let right_str = translate_having_condition(right, aggregates)?;
            Ok(format!("({left_str} || {right_str})"))
        }
        _ => Err(SqlError::unsupported(
            "Complex HAVING conditions are not supported",
        )),
    }
}

/// Resolve a HAVING field name to the actual alias used in REDUCE.
///
/// The HAVING clause parser generates field names like "count" or "sum_price" for
/// aggregate functions. This function matches those names against the SELECT
/// clause aggregates to find the correct alias.
fn resolve_having_field<'a>(field: &'a str, aggregates: &'a [AggregateExpr]) -> &'a str {
    // Try to find a matching aggregate
    for agg in aggregates {
        // Generate the default name that extract_having_field would produce
        // Use the same logic as generate_default_alias for base function names
        let base_name = agg.function.to_string().to_lowercase();
        let default_name = match &agg.field {
            Some(f) => format!("{base_name}_{f}"),
            None => base_name,
        };

        // If the field matches the default name, return the aggregate's alias
        if field == default_name {
            // Return the alias if present, otherwise the default name
            if let Some(alias) = &agg.alias {
                return alias.as_str();
            }
            // No alias - return the default name that REDUCE will use
            return field;
        }

        // Also check if the field directly matches an alias (user used alias in HAVING)
        if let Some(alias) = &agg.alias
            && field == alias
        {
            return alias.as_str();
        }
    }

    // No matching aggregate found, return as-is
    field
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{OrderBy, Value};

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

    // NOT operator tests
    #[test]
    fn test_translate_not_equals() {
        let query =
            SelectQuery::new("idx").with_condition(Condition::Not(Box::new(Condition::Equals {
                field: "category".to_string(),
                value: Value::String("electronics".to_string()),
            })));
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-(@category:{electronics})");
    }

    #[test]
    fn test_translate_not_greater_than() {
        let query = SelectQuery::new("idx").with_condition(Condition::Not(Box::new(
            Condition::GreaterThan {
                field: "price".to_string(),
                value: Value::Number(100.0),
            },
        )));
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-(@price:[(100 +inf])");
    }

    #[test]
    fn test_translate_not_like() {
        let query =
            SelectQuery::new("idx").with_condition(Condition::Not(Box::new(Condition::Like {
                field: "name".to_string(),
                pattern: "Lap%".to_string(),
                negated: false,
            })));
        let result = translate(query).unwrap();
        assert_eq!(result.query_string, "-(@name:Lap*)");
    }

    // Multiple ORDER BY tests
    #[test]
    fn test_translate_order_by_multiple_columns_search_rejected() {
        use crate::ast::OrderByColumn;
        let mut query = SelectQuery::new("products");
        query.order_by = Some(OrderBy {
            columns: vec![
                OrderByColumn {
                    field: "category".to_string(),
                    direction: crate::ast::SortDirection::Asc,
                },
                OrderByColumn {
                    field: "price".to_string(),
                    direction: crate::ast::SortDirection::Desc,
                },
            ],
        });
        // FT.SEARCH does not support multiple ORDER BY columns
        let result = translate(query);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message
                .contains("Multiple ORDER BY columns are not supported by FT.SEARCH")
        );
        assert!(err.suggestion.is_some());
    }

    #[test]
    fn test_translate_order_by_multiple_columns_aggregate() {
        use crate::ast::{AggregateExpr, AggregateFunction, OrderByColumn};
        let mut query = SelectQuery::new("products");
        // Add aggregate to trigger FT.AGGREGATE
        query.aggregates.push(AggregateExpr {
            function: AggregateFunction::Count,
            field: None,
            alias: Some("total".to_string()),
        });
        query.order_by = Some(OrderBy {
            columns: vec![
                OrderByColumn {
                    field: "category".to_string(),
                    direction: crate::ast::SortDirection::Asc,
                },
                OrderByColumn {
                    field: "price".to_string(),
                    direction: crate::ast::SortDirection::Desc,
                },
            ],
        });
        // FT.AGGREGATE supports multiple ORDER BY columns
        let result = translate(query).unwrap();
        assert_eq!(result.command, Command::Aggregate);
        // SORTBY nargs @field1 ASC @field2 DESC
        assert!(result.arguments.contains(&"SORTBY".to_string()));
        assert!(result.arguments.contains(&"4".to_string())); // nargs = 2 columns * 2
        assert!(result.arguments.contains(&"@category".to_string()));
        assert!(result.arguments.contains(&"ASC".to_string()));
        assert!(result.arguments.contains(&"@price".to_string()));
        assert!(result.arguments.contains(&"DESC".to_string()));
    }

    #[test]
    fn test_translate_order_by_single_column() {
        let mut query = SelectQuery::new("products");
        query.order_by = Some(OrderBy::single("name", crate::ast::SortDirection::Asc));
        let result = translate(query).unwrap();
        assert!(result.arguments.contains(&"SORTBY".to_string()));
        assert!(result.arguments.contains(&"name".to_string()));
        assert!(result.arguments.contains(&"ASC".to_string()));
    }
}
