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
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::ast::{
    AggregateExpr, AggregateFunction, Condition, DistanceMetric, GroupBy, HybridSearch, Limit,
    OrderBy, OrderByColumn, SelectField, SelectQuery, SortDirection, Value, VectorSearch,
};
use crate::error::SqlError;
use sqlparser::ast::{Distinct, FunctionArg, FunctionArgExpr, FunctionArguments, GroupByExpr};

/// Parsed OPTION clause values.
#[derive(Debug, Clone, Default)]
struct QueryOptions {
    /// Weight for vector scoring (0.0 to 1.0).
    vector_weight: Option<f64>,
    /// Weight for text scoring (0.0 to 1.0).
    text_weight: Option<f64>,
}

/// Parses a SQL query string into our internal AST representation.
pub fn parse(sql: &str) -> Result<SelectQuery, SqlError> {
    // Extract OPTION clause if present (non-standard SQL extension)
    let (sql_without_options, options) = extract_option_clause(sql)?;

    // Use PostgreSQL dialect to support <-> (L2 distance) operator from pgvector
    let dialect = PostgreSqlDialect {};
    let statements = Parser::parse_sql(&dialect, &sql_without_options)?;

    if statements.is_empty() {
        return Err(SqlError::syntax("Empty query"));
    }

    if statements.len() > 1 {
        return Err(SqlError::unsupported(
            "Multiple statements are not supported",
        ));
    }

    let statement = statements.into_iter().next().unwrap();
    let mut query = parse_statement(statement)?;

    // Apply OPTION clause to create HybridSearch if weights specified with vector search
    apply_hybrid_options(&mut query, &options)?;

    Ok(query)
}

/// Extract OPTION clause from SQL and return the SQL without it.
/// Syntax: OPTION (key = value, key = value, ...)
fn extract_option_clause(sql: &str) -> Result<(String, QueryOptions), SqlError> {
    let sql_upper = sql.to_uppercase();
    let option_idx = sql_upper.find(" OPTION ");
    // Also check for OPTION at end without leading space
    let option_idx = option_idx.or_else(|| {
        if sql_upper.ends_with(" OPTION") {
            Some(sql_upper.len() - 7)
        } else {
            None
        }
    });

    let Some(option_idx) = option_idx else {
        return Ok((sql.to_string(), QueryOptions::default()));
    };

    // Find the opening parenthesis
    let rest = &sql[option_idx + 7..].trim_start();
    if !rest.starts_with('(') {
        return Err(SqlError::syntax(
            "OPTION clause must be followed by parentheses: OPTION (key = value, ...)",
        ));
    }

    // Find the closing parenthesis
    let paren_start = sql.find('(').unwrap_or(option_idx);
    let paren_end = sql.rfind(')');
    let Some(paren_end) = paren_end else {
        return Err(SqlError::syntax(
            "OPTION clause: missing closing parenthesis",
        ));
    };

    // Extract the content between parentheses
    let options_content = &sql[paren_start + 1..paren_end];
    let options = parse_option_content(options_content)?;

    // Return SQL without the OPTION clause
    let sql_without_options = sql[..option_idx].trim().to_string();

    Ok((sql_without_options, options))
}

/// Parse the content inside OPTION (...).
fn parse_option_content(content: &str) -> Result<QueryOptions, SqlError> {
    let mut options = QueryOptions::default();

    for pair in content.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(SqlError::syntax(format!(
                "Invalid OPTION format: '{}'. Expected 'key = value'",
                pair
            )));
        }

        let key = parts[0].trim().to_lowercase();
        let value = parts[1].trim();

        match key.as_str() {
            "vector_weight" => {
                options.vector_weight = Some(parse_weight_value(value, "vector_weight")?);
            }
            "text_weight" => {
                options.text_weight = Some(parse_weight_value(value, "text_weight")?);
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "Unknown OPTION key: '{}'. Supported keys: vector_weight, text_weight",
                    key
                )));
            }
        }
    }

    Ok(options)
}

/// Parse a weight value (must be between 0.0 and 1.0).
fn parse_weight_value(value: &str, name: &str) -> Result<f64, SqlError> {
    let weight: f64 = value.parse().map_err(|_| {
        SqlError::syntax(format!(
            "{} must be a number between 0.0 and 1.0, got: '{}'",
            name, value
        ))
    })?;

    if !(0.0..=1.0).contains(&weight) {
        return Err(SqlError::syntax(format!(
            "{} must be between 0.0 and 1.0, got: {}",
            name, weight
        )));
    }

    Ok(weight)
}

/// Apply OPTION clause to create HybridSearch if weights are specified.
fn apply_hybrid_options(query: &mut SelectQuery, options: &QueryOptions) -> Result<(), SqlError> {
    // Only create HybridSearch if weights are specified AND there's a vector search
    if options.vector_weight.is_none() && options.text_weight.is_none() {
        return Ok(());
    }

    // Must have vector search to use weights
    let vector_search = match query.vector_search.take() {
        Some(vs) => vs,
        None => {
            return Err(SqlError::syntax(
                "OPTION with weights requires a vector search (ORDER BY field <-> vector)",
            ));
        }
    };

    // Set defaults if not specified
    let vector_weight = options.vector_weight.unwrap_or(0.5);
    let text_weight = options.text_weight.unwrap_or(0.5);

    // Create hybrid search configuration
    query.hybrid_search = Some(HybridSearch {
        vector: vector_search,
        vector_weight,
        text_weight,
    });

    Ok(())
}

fn parse_statement(statement: Statement) -> Result<SelectQuery, SqlError> {
    match statement {
        Statement::Query(query) => {
            let select = match *query.body {
                SetExpr::Select(select) => select,
                _ => {
                    return Err(SqlError::unsupported("Only SELECT queries are supported"));
                }
            };

            // Check for DISTINCT
            let distinct = matches!(select.distinct, Some(Distinct::Distinct));
            if matches!(select.distinct, Some(Distinct::On(_))) {
                return Err(SqlError::unsupported(
                    "DISTINCT ON is not supported, use DISTINCT instead",
                ));
            }

            // Parse fields and aggregates from SELECT clause
            let SelectItemsResult { fields, aggregates } = parse_select_items(&select.projection)?;

            // Parse FROM clause
            let index_name = parse_from_clause(&select.from)?;

            // Parse WHERE clause
            let conditions = if let Some(selection) = select.selection {
                parse_where_clause(&selection)?
            } else {
                Vec::new()
            };

            // Parse GROUP BY clause
            let group_by = parse_group_by(&select.group_by)?;

            // Parse HAVING clause
            let having = if let Some(having_expr) = &select.having {
                Some(parse_having_expression(having_expr)?)
            } else {
                None
            };

            // Parse LIMIT/OFFSET clause first (needed for vector K)
            let limit = parse_limit(query.limit.as_ref(), query.offset.as_ref())?;

            // Parse ORDER BY clause (may contain vector search)
            let OrderByResult {
                order_by,
                vector_search,
            } = if let Some(ref ob) = query.order_by {
                parse_order_by(&ob.exprs, limit.as_ref().map(|l| l.count as usize))?
            } else {
                OrderByResult {
                    order_by: None,
                    vector_search: None,
                }
            };

            Ok(SelectQuery {
                fields,
                index_name,
                conditions,
                order_by,
                limit,
                distinct,
                aggregates,
                group_by,
                having,
                vector_search,
                hybrid_search: None, // TODO: Parse HYBRID syntax
            })
        }
        _ => Err(SqlError::unsupported(
            "Only SELECT statements are supported",
        )),
    }
}

/// Result of parsing SELECT items.
struct SelectItemsResult {
    fields: Vec<SelectField>,
    aggregates: Vec<AggregateExpr>,
}

fn parse_select_items(items: &[SelectItem]) -> Result<SelectItemsResult, SqlError> {
    let mut fields = Vec::new();
    let mut aggregates = Vec::new();

    for item in items {
        match item {
            SelectItem::Wildcard(_) => {
                // SELECT * - return empty vec to indicate all fields
                return Ok(SelectItemsResult {
                    fields: Vec::new(),
                    aggregates: Vec::new(),
                });
            }
            SelectItem::UnnamedExpr(Expr::Function(func)) => {
                if let Some(agg) = parse_aggregate_function(func, None)? {
                    aggregates.push(agg);
                } else {
                    return Err(SqlError::unsupported(format!(
                        "Unsupported function in SELECT: {}",
                        func.name
                    )));
                }
            }
            SelectItem::UnnamedExpr(expr) => {
                let field_name = extract_identifier(expr)?;
                fields.push(SelectField {
                    name: field_name,
                    alias: None,
                });
            }
            SelectItem::ExprWithAlias {
                expr: Expr::Function(func),
                alias,
            } => {
                if let Some(agg) = parse_aggregate_function(func, Some(alias.value.clone()))? {
                    aggregates.push(agg);
                } else {
                    return Err(SqlError::unsupported(format!(
                        "Unsupported function in SELECT: {}",
                        func.name
                    )));
                }
            }
            SelectItem::ExprWithAlias { expr, alias } => {
                let field_name = extract_identifier(expr)?;
                fields.push(SelectField {
                    name: field_name,
                    alias: Some(alias.value.clone()),
                });
            }
            _ => {
                return Err(SqlError::unsupported(format!(
                    "Unsupported SELECT item: {item:?}"
                )));
            }
        }
    }

    Ok(SelectItemsResult { fields, aggregates })
}

/// Parse an aggregate function from a SQL function expression.
fn parse_aggregate_function(
    func: &sqlparser::ast::Function,
    alias: Option<String>,
) -> Result<Option<AggregateExpr>, SqlError> {
    let name = func.name.to_string().to_uppercase();
    let args = extract_function_args(func)?;

    // Match simple single-argument aggregates
    let result = match name.as_str() {
        "COUNT" => {
            let field = if args.is_empty() || args.first().map(|a| a.as_str()) == Some("*") {
                None
            } else {
                Some(args.into_iter().next().unwrap())
            };
            Some((AggregateFunction::Count, field))
        }
        "SUM" => Some((AggregateFunction::Sum, args.into_iter().next())),
        "AVG" => Some((AggregateFunction::Avg, args.into_iter().next())),
        "MIN" => Some((AggregateFunction::Min, args.into_iter().next())),
        "MAX" => Some((AggregateFunction::Max, args.into_iter().next())),
        "COUNT_DISTINCT" => Some((AggregateFunction::CountDistinct, args.into_iter().next())),
        "COUNT_DISTINCTISH" => Some((AggregateFunction::CountDistinctish, args.into_iter().next())),
        "STDDEV" => Some((AggregateFunction::Stddev, args.into_iter().next())),
        "TOLIST" => Some((AggregateFunction::Tolist, args.into_iter().next())),
        "HLL" => Some((AggregateFunction::Hll, args.into_iter().next())),
        "HLL_SUM" => Some((AggregateFunction::HllSum, args.into_iter().next())),
        "QUANTILE" => {
            // QUANTILE(field, percentile) - requires 2 arguments
            if args.len() != 2 {
                return Err(SqlError::syntax(
                    "QUANTILE requires exactly 2 arguments: QUANTILE(field, percentile)",
                ));
            }
            let field = args[0].clone();
            let percentile: f64 = args[1].parse().map_err(|_| {
                SqlError::syntax(format!(
                    "QUANTILE percentile must be a number between 0.0 and 1.0, got: '{}'",
                    args[1]
                ))
            })?;
            if !(0.0..=1.0).contains(&percentile) {
                return Err(SqlError::syntax(format!(
                    "QUANTILE percentile must be between 0.0 and 1.0, got: {}",
                    percentile
                )));
            }
            Some((AggregateFunction::Quantile { percentile }, Some(field)))
        }
        "RANDOM_SAMPLE" => {
            // RANDOM_SAMPLE(field, size) - requires 2 arguments
            if args.len() != 2 {
                return Err(SqlError::syntax(
                    "RANDOM_SAMPLE requires exactly 2 arguments: RANDOM_SAMPLE(field, size)",
                ));
            }
            let field = args[0].clone();
            let size: u32 = args[1].parse().map_err(|_| {
                SqlError::syntax(format!(
                    "RANDOM_SAMPLE size must be a positive integer, got: '{}'",
                    args[1]
                ))
            })?;
            if size == 0 || size > 1000 {
                return Err(SqlError::syntax(format!(
                    "RANDOM_SAMPLE size must be between 1 and 1000, got: {}",
                    size
                )));
            }
            Some((AggregateFunction::RandomSample { size }, Some(field)))
        }
        "FIRST_VALUE" => {
            // FIRST_VALUE(field, sort_field) or FIRST_VALUE(field, sort_field, 'ASC'/'DESC')
            // Simplified syntax since SQL standard window function syntax is complex
            if args.len() < 2 || args.len() > 3 {
                return Err(SqlError::syntax(
                    "FIRST_VALUE requires 2-3 arguments: FIRST_VALUE(field, sort_field [, 'ASC'|'DESC'])",
                ));
            }
            let field = args[0].clone();
            let sort_field = args[1].clone();
            let ascending = if args.len() == 3 {
                match args[2].to_uppercase().as_str() {
                    "ASC" => true,
                    "DESC" => false,
                    _ => {
                        return Err(SqlError::syntax(
                            "FIRST_VALUE third argument must be 'ASC' or 'DESC'",
                        ));
                    }
                }
            } else {
                false // Default to DESC as per RediSearch convention
            };
            Some((
                AggregateFunction::FirstValue {
                    sort_field,
                    ascending,
                },
                Some(field),
            ))
        }
        _ => None,
    };

    match result {
        Some((function, field)) => Ok(Some(AggregateExpr {
            function,
            field,
            alias,
        })),
        None => Ok(None),
    }
}

/// Extract function arguments as strings.
fn extract_function_args(func: &sqlparser::ast::Function) -> Result<Vec<String>, SqlError> {
    match &func.args {
        FunctionArguments::List(arg_list) => {
            let mut args = Vec::new();
            for arg in &arg_list.args {
                match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {
                        args.push("*".to_string());
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                        // Handle numeric literals, identifiers, and string literals
                        match expr {
                            Expr::Identifier(ident) => args.push(ident.value.clone()),
                            Expr::Value(sqlparser::ast::Value::Number(n, _)) => {
                                args.push(n.clone())
                            }
                            Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
                            | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => {
                                args.push(s.clone())
                            }
                            Expr::CompoundIdentifier(parts) => {
                                if let Some(last) = parts.last() {
                                    args.push(last.value.clone());
                                }
                            }
                            _ => {
                                return Err(SqlError::unsupported(format!(
                                    "Unsupported function argument expression: {expr:?}"
                                )));
                            }
                        }
                    }
                    _ => {
                        return Err(SqlError::unsupported("Unsupported function argument type"));
                    }
                }
            }
            Ok(args)
        }
        FunctionArguments::None => Ok(Vec::new()),
        FunctionArguments::Subquery(_) => Err(SqlError::unsupported(
            "Subqueries in aggregate functions are not supported",
        )),
    }
}

/// Parse GROUP BY clause.
fn parse_group_by(group_by: &GroupByExpr) -> Result<Option<GroupBy>, SqlError> {
    match group_by {
        GroupByExpr::All(_) => Err(SqlError::unsupported("GROUP BY ALL is not supported")),
        GroupByExpr::Expressions(exprs, _) => {
            if exprs.is_empty() {
                return Ok(None);
            }
            let fields: Vec<String> = exprs
                .iter()
                .map(extract_identifier)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Some(GroupBy { fields }))
        }
    }
}

/// Parse HAVING expression (supports aggregate functions and AND/OR).
fn parse_having_expression(expr: &Expr) -> Result<Condition, SqlError> {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            // Handle AND/OR logical operators
            match op {
                BinaryOperator::And => {
                    let left_cond = parse_having_expression(left)?;
                    let right_cond = parse_having_expression(right)?;
                    return Ok(Condition::And(Box::new(left_cond), Box::new(right_cond)));
                }
                BinaryOperator::Or => {
                    let left_cond = parse_having_expression(left)?;
                    let right_cond = parse_having_expression(right)?;
                    return Ok(Condition::Or(Box::new(left_cond), Box::new(right_cond)));
                }
                _ => {}
            }

            // In HAVING, the left side is typically an aggregate function or alias
            let field = extract_having_field(left)?;
            let value = extract_value(right)?;

            match op {
                BinaryOperator::Gt => Ok(Condition::GreaterThan { field, value }),
                BinaryOperator::GtEq => Ok(Condition::GreaterThanOrEqual { field, value }),
                BinaryOperator::Lt => Ok(Condition::LessThan { field, value }),
                BinaryOperator::LtEq => Ok(Condition::LessThanOrEqual { field, value }),
                BinaryOperator::Eq => Ok(Condition::Equals { field, value }),
                BinaryOperator::NotEq => Ok(Condition::NotEquals { field, value }),
                _ => Err(SqlError::unsupported(format!(
                    "Operator {op:?} not supported in HAVING clause"
                ))),
            }
        }
        Expr::Nested(inner) => {
            // Handle parenthesized expressions like (COUNT(*) > 5)
            parse_having_expression(inner)
        }
        _ => Err(SqlError::unsupported(
            "Only simple comparisons are supported in HAVING clause",
        )),
    }
}

/// Extract field name from HAVING expression (handles aggregates).
fn extract_having_field(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Identifier(ident) => Ok(ident.value.clone()),
        Expr::Function(func) => {
            // For aggregate functions like COUNT(*), generate a field name
            let func_name = func.name.to_string().to_uppercase();
            match &func.args {
                FunctionArguments::List(arg_list) => {
                    if arg_list.args.is_empty()
                        || matches!(
                            arg_list.args.first(),
                            Some(FunctionArg::Unnamed(FunctionArgExpr::Wildcard))
                        )
                    {
                        Ok(func_name.to_lowercase())
                    } else if arg_list.args.len() == 1 {
                        if let Some(FunctionArg::Unnamed(FunctionArgExpr::Expr(e))) =
                            arg_list.args.first()
                        {
                            let field = extract_identifier(e)?;
                            Ok(format!("{}_{}", func_name.to_lowercase(), field))
                        } else {
                            Ok(func_name.to_lowercase())
                        }
                    } else {
                        Ok(func_name.to_lowercase())
                    }
                }
                _ => Ok(func_name.to_lowercase()),
            }
        }
        _ => Err(SqlError::unsupported(format!(
            "Unsupported expression in HAVING: {expr:?}"
        ))),
    }
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
        Expr::Like {
            expr,
            pattern,
            negated,
            ..
        } => {
            let field = extract_identifier(expr)?;
            let pattern_str = extract_string_value(pattern)?;
            conditions.push(Condition::Like {
                field,
                pattern: pattern_str,
                negated: *negated,
            });
            Ok(())
        }
        Expr::IsNull(expr) => {
            let field = extract_identifier(expr)?;
            conditions.push(Condition::IsNull {
                field,
                negated: false,
            });
            Ok(())
        }
        Expr::IsNotNull(expr) => {
            let field = extract_identifier(expr)?;
            conditions.push(Condition::IsNull {
                field,
                negated: true,
            });
            Ok(())
        }
        Expr::UnaryOp {
            op: sqlparser::ast::UnaryOperator::Not,
            expr: inner,
        } => {
            // Handle NOT (condition)
            let mut inner_conditions = Vec::new();
            parse_expression(inner, &mut inner_conditions)?;
            if inner_conditions.len() == 1 {
                let inner_cond = inner_conditions.pop().unwrap();
                conditions.push(Condition::Not(Box::new(inner_cond)));
                Ok(())
            } else {
                Err(SqlError::unsupported(
                    "NOT with multiple conditions is not supported",
                ))
            }
        }
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
        BinaryOperator::NotEq => {
            let field = extract_identifier(left)?;
            let value = extract_value(right)?;
            conditions.push(Condition::NotEquals { field, value });
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
        BinaryOperator::Or => {
            // For OR, we need to create a combined OR condition
            let mut left_conditions = Vec::new();
            let mut right_conditions = Vec::new();
            parse_expression(left, &mut left_conditions)?;
            parse_expression(right, &mut right_conditions)?;

            // Each side should have exactly one condition for simple OR
            if left_conditions.len() == 1 && right_conditions.len() == 1 {
                let left_cond = left_conditions.pop().unwrap();
                let right_cond = right_conditions.pop().unwrap();
                conditions.push(Condition::Or(Box::new(left_cond), Box::new(right_cond)));
            } else {
                // Multiple conditions on one side - we support this by combining
                // For (a AND b) OR c, we can't easily translate to RQL, but we'll try
                return Err(SqlError::unsupported(
                    "Complex OR expressions with multiple conditions on one side are not supported",
                ));
            }
            Ok(())
        }
        _ => Err(SqlError::unsupported(format!(
            "Operator {op:?} is not supported"
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

fn extract_string_value(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
        | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => Ok(s.clone()),
        _ => Err(SqlError::translation(format!(
            "Expected string value, got: {expr:?}"
        ))),
    }
}

/// Result of parsing ORDER BY clause.
struct OrderByResult {
    /// Standard ORDER BY clause.
    order_by: Option<OrderBy>,
    /// Vector search extracted from ORDER BY field <-> 'vector'.
    vector_search: Option<VectorSearch>,
}

fn parse_order_by(
    order_by: &[OrderByExpr],
    limit: Option<usize>,
) -> Result<OrderByResult, SqlError> {
    if order_by.is_empty() {
        return Ok(OrderByResult {
            order_by: None,
            vector_search: None,
        });
    }

    // Check first expression for vector distance operator
    let first_expr = &order_by[0];
    if let Some(vector_search) = try_parse_vector_order_by(&first_expr.expr, limit)? {
        // Vector search found - additional ORDER BY columns not supported with vector search
        if order_by.len() > 1 {
            return Err(SqlError::unsupported(
                "Multiple ORDER BY columns are not supported with vector search",
            ));
        }
        return Ok(OrderByResult {
            order_by: None,
            vector_search: Some(vector_search),
        });
    }

    // Parse all ORDER BY columns
    let mut columns = Vec::with_capacity(order_by.len());
    for order_expr in order_by {
        let field = extract_identifier(&order_expr.expr)?;
        let direction = if order_expr.asc.unwrap_or(true) {
            SortDirection::Asc
        } else {
            SortDirection::Desc
        };
        columns.push(OrderByColumn { field, direction });
    }

    Ok(OrderByResult {
        order_by: Some(OrderBy { columns }),
        vector_search: None,
    })
}

/// Try to parse vector distance operator from ORDER BY expression.
fn try_parse_vector_order_by(
    expr: &Expr,
    limit: Option<usize>,
) -> Result<Option<VectorSearch>, SqlError> {
    // Vector distance uses the <-> (L2), <=> (Cosine), or <#> (IP) operators
    // e.g., embedding <-> '[0.1, 0.2, 0.3]'
    match expr {
        Expr::BinaryOp { left, op, right } => {
            // Check if this is a distance operator and get the metric
            let distance_metric = match get_vector_distance_metric(op) {
                Some(metric) => metric,
                None => return Ok(None),
            };

            let field = extract_identifier(left)?;
            let vector = extract_vector_value(right)?;

            // K is determined by LIMIT, default to 10
            let k = limit.unwrap_or(10);

            Ok(Some(VectorSearch {
                field,
                vector,
                k,
                distance_metric,
            }))
        }
        _ => Ok(None),
    }
}

/// Get the distance metric for a vector distance operator.
/// Returns Some(metric) for <-> (L2), <=> (Cosine), <#> (IP), None otherwise.
fn get_vector_distance_metric(op: &BinaryOperator) -> Option<DistanceMetric> {
    match op {
        // <=> is parsed as Spaceship by sqlparser-rs (SQL:2023 standard operator)
        BinaryOperator::Spaceship => Some(DistanceMetric::Cosine),
        BinaryOperator::ArrowAt => Some(DistanceMetric::L2), // <@ (similar pattern)
        BinaryOperator::PGCustomBinaryOperator(parts) => {
            // Custom operator like OPERATOR(<->), OPERATOR(<=>), OPERATOR(<#>)
            let op_str: String = parts.iter().map(|p| p.to_string()).collect();
            if op_str == "<->" || op_str.contains("<->") {
                Some(DistanceMetric::L2)
            } else if op_str == "<=>" || op_str.contains("<=>") {
                Some(DistanceMetric::Cosine)
            } else if op_str == "<#>" || op_str.contains("<#>") {
                Some(DistanceMetric::InnerProduct)
            } else {
                None
            }
        }
        _ => {
            // Check debug string as fallback
            let debug_str = format!("{op:?}");
            if debug_str.contains("<->") {
                Some(DistanceMetric::L2)
            } else if debug_str.contains("<=>") || debug_str.contains("Spaceship") {
                Some(DistanceMetric::Cosine)
            } else if debug_str.contains("<#>") {
                Some(DistanceMetric::InnerProduct)
            } else if debug_str.contains("Arrow") {
                Some(DistanceMetric::L2) // Default for Arrow-like operators
            } else {
                None
            }
        }
    }
}

/// Extract a vector value from an expression (string representation).
fn extract_vector_value(expr: &Expr) -> Result<String, SqlError> {
    match expr {
        Expr::Value(sqlparser::ast::Value::SingleQuotedString(s))
        | Expr::Value(sqlparser::ast::Value::DoubleQuotedString(s)) => Ok(s.clone()),
        Expr::Array(arr) => {
            // Handle array literal like [0.1, 0.2, 0.3]
            let elements: Vec<String> = arr
                .elem
                .iter()
                .map(|e| match e {
                    Expr::Value(sqlparser::ast::Value::Number(n, _)) => Ok(n.clone()),
                    _ => Err(SqlError::translation("Vector elements must be numbers")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(format!("[{}]", elements.join(",")))
        }
        _ => Err(SqlError::translation(format!(
            "Expected vector value (string or array), got: {expr:?}"
        ))),
    }
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
        assert_eq!(query.fields.len(), 1);
        assert_eq!(query.fields[0].name, "name");
    }

    // ORDER BY tests
    #[test]
    fn test_parse_order_by_asc() {
        let query = parse("SELECT * FROM idx ORDER BY name ASC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.columns.len(), 1);
        assert_eq!(order_by.columns[0].field, "name");
        assert_eq!(order_by.columns[0].direction, SortDirection::Asc);
    }

    #[test]
    fn test_parse_order_by_desc() {
        let query = parse("SELECT * FROM idx ORDER BY price DESC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.columns.len(), 1);
        assert_eq!(order_by.columns[0].field, "price");
        assert_eq!(order_by.columns[0].direction, SortDirection::Desc);
    }

    #[test]
    fn test_parse_order_by_default_asc() {
        // Default should be ASC when not specified
        let query = parse("SELECT * FROM idx ORDER BY name").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.columns[0].direction, SortDirection::Asc);
    }

    #[test]
    fn test_parse_order_by_multiple_columns() {
        let query = parse("SELECT * FROM idx ORDER BY category ASC, price DESC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.columns.len(), 2);
        assert_eq!(order_by.columns[0].field, "category");
        assert_eq!(order_by.columns[0].direction, SortDirection::Asc);
        assert_eq!(order_by.columns[1].field, "price");
        assert_eq!(order_by.columns[1].direction, SortDirection::Desc);
    }

    #[test]
    fn test_parse_order_by_three_columns() {
        let query = parse("SELECT * FROM idx ORDER BY category ASC, price DESC, name ASC").unwrap();
        let order_by = query.order_by.unwrap();
        assert_eq!(order_by.columns.len(), 3);
        assert_eq!(order_by.columns[0].field, "category");
        assert_eq!(order_by.columns[1].field, "price");
        assert_eq!(order_by.columns[2].field, "name");
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
    fn test_parse_not_between() {
        let result = parse("SELECT * FROM idx WHERE x NOT BETWEEN 1 AND 10");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("NOT BETWEEN"));
    }

    #[test]
    fn test_parse_or_operator_supported() {
        let query = parse("SELECT * FROM idx WHERE a = 1 OR b = 2").unwrap();
        assert_eq!(query.conditions.len(), 1);
        assert!(matches!(&query.conditions[0], Condition::Or(_, _)));
    }

    #[test]
    fn test_parse_unsupported_expression() {
        // Subquery is not supported
        let result = parse("SELECT * FROM idx WHERE id IN (SELECT id FROM other)");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_count_star() {
        let query = parse("SELECT COUNT(*) FROM idx").unwrap();
        assert!(query.fields.is_empty());
        assert_eq!(query.aggregates.len(), 1);
        assert!(matches!(
            query.aggregates[0].function,
            crate::ast::AggregateFunction::Count
        ));
        assert!(query.aggregates[0].field.is_none()); // COUNT(*) has no field
    }

    #[test]
    fn test_parse_aggregate_sum() {
        let query = parse("SELECT SUM(price) FROM idx").unwrap();
        assert_eq!(query.aggregates.len(), 1);
        assert!(matches!(
            query.aggregates[0].function,
            crate::ast::AggregateFunction::Sum
        ));
        assert_eq!(query.aggregates[0].field, Some("price".to_string()));
    }

    #[test]
    fn test_parse_group_by() {
        let query = parse("SELECT category, COUNT(*) FROM idx GROUP BY category").unwrap();
        assert!(query.group_by.is_some());
        let group_by = query.group_by.unwrap();
        assert_eq!(group_by.fields, vec!["category"]);
    }

    #[test]
    fn test_parse_distinct() {
        let query = parse("SELECT DISTINCT category FROM idx").unwrap();
        assert!(query.distinct);
        assert_eq!(query.fields.len(), 1);
    }

    #[test]
    fn test_parse_not_equals() {
        let query = parse("SELECT * FROM idx WHERE status != 'deleted'").unwrap();
        assert_eq!(query.conditions.len(), 1);
        assert!(matches!(&query.conditions[0], Condition::NotEquals { .. }));
    }

    #[test]
    fn test_parse_like() {
        let query = parse("SELECT * FROM idx WHERE name LIKE 'Lap%'").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::Like {
                field,
                pattern,
                negated,
            } => {
                assert_eq!(field, "name");
                assert_eq!(pattern, "Lap%");
                assert!(!negated);
            }
            _ => panic!("Expected Like condition"),
        }
    }

    #[test]
    fn test_parse_is_null() {
        let query = parse("SELECT * FROM idx WHERE category IS NULL").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::IsNull { field, negated } => {
                assert_eq!(field, "category");
                assert!(!negated);
            }
            _ => panic!("Expected IsNull condition"),
        }
    }

    #[test]
    fn test_parse_is_not_null() {
        let query = parse("SELECT * FROM idx WHERE category IS NOT NULL").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::IsNull { field, negated } => {
                assert_eq!(field, "category");
                assert!(*negated);
            }
            _ => panic!("Expected IsNull condition with negated=true"),
        }
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

    // Vector search tests
    #[test]
    fn test_parse_vector_search_basic() {
        let query =
            parse("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert!(query.vector_search.is_some());
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.field, "embedding");
        assert_eq!(vs.vector, "[0.1, 0.2, 0.3]");
        assert_eq!(vs.k, 10);
    }

    #[test]
    fn test_parse_vector_search_with_filter() {
        let query = parse(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 5",
        )
        .unwrap();
        assert!(query.vector_search.is_some());
        assert_eq!(query.conditions.len(), 1);
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.k, 5);
    }

    #[test]
    fn test_parse_vector_search_default_k() {
        // Without LIMIT, should default to k=10
        let query =
            parse("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]'").unwrap();
        assert!(query.vector_search.is_some());
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.k, 10);
    }

    #[test]
    fn test_parse_vector_search_l2_distance_metric() {
        // <-> operator should use L2 metric
        let query =
            parse("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert!(query.vector_search.is_some());
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.distance_metric, DistanceMetric::L2);
    }

    #[test]
    fn test_parse_vector_search_cosine_distance() {
        // <=> operator should use Cosine metric
        let query =
            parse("SELECT * FROM products ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert!(query.vector_search.is_some());
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.field, "embedding");
        assert_eq!(vs.vector, "[0.1, 0.2, 0.3]");
        assert_eq!(vs.k, 10);
        assert_eq!(vs.distance_metric, DistanceMetric::Cosine);
    }

    #[test]
    fn test_parse_vector_search_cosine_with_filter() {
        // Cosine distance with WHERE filter
        let query = parse(
            "SELECT * FROM products WHERE category = 'electronics' ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5",
        )
        .unwrap();
        assert!(query.vector_search.is_some());
        assert_eq!(query.conditions.len(), 1);
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.k, 5);
        assert_eq!(vs.distance_metric, DistanceMetric::Cosine);
    }

    #[test]
    fn test_parse_vector_search_inner_product() {
        // <#> operator should use InnerProduct metric
        let query =
            parse("SELECT * FROM products ORDER BY embedding <#> '[0.1, 0.2, 0.3]' LIMIT 10")
                .unwrap();
        assert!(query.vector_search.is_some());
        let vs = query.vector_search.unwrap();
        assert_eq!(vs.field, "embedding");
        assert_eq!(vs.distance_metric, DistanceMetric::InnerProduct);
    }

    // OPTION clause tests for FT.HYBRID with weights
    #[test]
    fn test_parse_option_clause_basic() {
        // OPTION with vector and text weights
        let query = parse(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10 \
             OPTION (vector_weight = 0.7, text_weight = 0.3)",
        )
        .unwrap();

        // With OPTION, vector_search becomes hybrid_search
        assert!(query.vector_search.is_none());
        assert!(query.hybrid_search.is_some());

        let hs = query.hybrid_search.unwrap();
        assert_eq!(hs.vector.field, "embedding");
        assert_eq!(hs.vector.k, 10);
        assert!((hs.vector_weight - 0.7).abs() < f64::EPSILON);
        assert!((hs.text_weight - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_option_clause_cosine() {
        // OPTION works with cosine distance too
        let query = parse(
            "SELECT * FROM products \
             ORDER BY embedding <=> '[0.1, 0.2]' LIMIT 5 \
             OPTION (vector_weight = 0.5, text_weight = 0.5)",
        )
        .unwrap();

        assert!(query.hybrid_search.is_some());
        let hs = query.hybrid_search.unwrap();
        assert_eq!(hs.vector.distance_metric, DistanceMetric::Cosine);
        assert_eq!(hs.vector.k, 5);
    }

    #[test]
    fn test_parse_option_clause_with_filter() {
        // OPTION with WHERE clause
        let query = parse(
            "SELECT * FROM products \
             WHERE category = 'electronics' \
             ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10 \
             OPTION (vector_weight = 0.8, text_weight = 0.2)",
        )
        .unwrap();

        assert!(query.hybrid_search.is_some());
        assert_eq!(query.conditions.len(), 1);

        let hs = query.hybrid_search.unwrap();
        assert!((hs.vector_weight - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_option_clause_default_weights() {
        // When only one weight is specified, other defaults to 0.5
        let query = parse(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1]' LIMIT 10 \
             OPTION (vector_weight = 0.6)",
        )
        .unwrap();

        let hs = query.hybrid_search.unwrap();
        assert!((hs.vector_weight - 0.6).abs() < f64::EPSILON);
        assert!((hs.text_weight - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_option_clause_error_no_vector() {
        // OPTION with weights requires vector search
        let result = parse(
            "SELECT * FROM products \
             WHERE category = 'electronics' \
             OPTION (vector_weight = 0.7)",
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("vector search"));
    }

    #[test]
    fn test_parse_option_clause_error_invalid_weight() {
        // Weight must be between 0.0 and 1.0
        let result = parse(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1]' LIMIT 10 \
             OPTION (vector_weight = 1.5)",
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("0.0 and 1.0"));
    }

    #[test]
    fn test_parse_option_clause_error_unknown_key() {
        // Unknown OPTION key
        let result = parse(
            "SELECT * FROM products \
             ORDER BY embedding <-> '[0.1]' LIMIT 10 \
             OPTION (unknown_option = 0.5)",
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown OPTION key"));
    }

    // NOT operator tests
    #[test]
    fn test_parse_not_equals_condition() {
        let query = parse("SELECT * FROM idx WHERE NOT (category = 'electronics')").unwrap();
        assert_eq!(query.conditions.len(), 1);
        assert!(matches!(&query.conditions[0], Condition::Not(_)));
    }

    #[test]
    fn test_parse_not_greater_than() {
        let query = parse("SELECT * FROM idx WHERE NOT (price > 100)").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::Not(inner) => {
                assert!(matches!(inner.as_ref(), Condition::GreaterThan { .. }));
            }
            _ => panic!("Expected Not condition"),
        }
    }

    #[test]
    fn test_parse_not_like() {
        let query = parse("SELECT * FROM idx WHERE NOT (name LIKE 'Lap%')").unwrap();
        assert_eq!(query.conditions.len(), 1);
        match &query.conditions[0] {
            Condition::Not(inner) => {
                assert!(matches!(inner.as_ref(), Condition::Like { .. }));
            }
            _ => panic!("Expected Not condition"),
        }
    }

    #[test]
    fn test_parse_not_with_and() {
        let query =
            parse("SELECT * FROM idx WHERE NOT (category = 'electronics') AND price > 50").unwrap();
        assert_eq!(query.conditions.len(), 2);
        assert!(matches!(&query.conditions[0], Condition::Not(_)));
        assert!(matches!(
            &query.conditions[1],
            Condition::GreaterThan { .. }
        ));
    }
}
