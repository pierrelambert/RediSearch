/*
 * Copyright (c) 2006-Present, Redis Ltd.
 * All rights reserved.
 *
 * Licensed under your choice of the Redis Source Available License 2.0
 * (RSALv2); or (b) the Server Side Public License v1 (SSPLv1); or (c) the
 * GNU Affero General Public License v3 (AGPLv3).
*/

//! SQL parsing wrapper using sqlparser crate.

use crate::error::SqlError;

use super::QueryOptions;

pub(super) fn quote_nonstandard_from_identifier(sql: &str) -> String {
    let Some(from_start) = find_keyword_outside_quotes(sql, "from") else {
        return sql.to_string();
    };

    let relation_start = sql[from_start + 4..]
        .char_indices()
        .find_map(|(offset, ch)| (!ch.is_whitespace()).then_some(from_start + 4 + offset));
    let Some(relation_start) = relation_start else {
        return sql.to_string();
    };

    if sql[relation_start..].starts_with('"') {
        return sql.to_string();
    }

    let relation_end = sql[relation_start..]
        .char_indices()
        .find_map(|(offset, ch)| is_relation_terminator(ch).then_some(relation_start + offset))
        .unwrap_or(sql.len());

    let relation_name = &sql[relation_start..relation_end];
    if !relation_name.chars().any(needs_identifier_quoting) {
        return sql.to_string();
    }

    let mut rewritten = String::with_capacity(sql.len() + 2);
    rewritten.push_str(&sql[..relation_start]);
    rewritten.push('"');
    rewritten.push_str(relation_name);
    rewritten.push('"');
    rewritten.push_str(&sql[relation_end..]);
    rewritten
}

fn find_keyword_outside_quotes(sql: &str, keyword: &str) -> Option<usize> {
    let keyword_len = keyword.len();
    let lowercase_sql = sql.to_ascii_lowercase();
    let bytes = lowercase_sql.as_bytes();
    let keyword_bytes = keyword.as_bytes();

    let mut in_single_quote = false;
    let mut in_double_quote = false;

    let mut i = 0;
    while i + keyword_len <= bytes.len() {
        match bytes[i] {
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                i += 1;
                continue;
            }
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                i += 1;
                continue;
            }
            _ => {}
        }

        if in_single_quote || in_double_quote {
            i += 1;
            continue;
        }

        if &bytes[i..i + keyword_len] == keyword_bytes {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
            let next_ok = i + keyword_len == bytes.len()
                || !bytes[i + keyword_len].is_ascii_alphanumeric()
                    && bytes[i + keyword_len] != b'_';
            if prev_ok && next_ok {
                return Some(i);
            }
        }

        i += 1;
    }

    None
}

const fn is_relation_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, ',' | ';' | ')')
}

const fn needs_identifier_quoting(ch: char) -> bool {
    !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' || ch == '.')
}

/// Extract OPTION clause from SQL and return the SQL without it.
/// Syntax: OPTION (key = value, key = value, ...)
pub(super) fn extract_option_clause(sql: &str) -> Result<(String, QueryOptions), SqlError> {
    let option_idx = find_option_keyword(sql);
    let Some(option_idx) = option_idx else {
        return Ok((sql.to_string(), QueryOptions::default()));
    };

    let option_end = option_idx + "OPTION".len();

    // Find the opening parenthesis
    let rest = &sql[option_end..].trim_start();
    if !rest.starts_with('(') {
        return Err(SqlError::syntax(
            "OPTION clause must be followed by parentheses: OPTION (key = value, ...)",
        ));
    }

    let paren_start = option_end + sql[option_end..].find('(').unwrap();
    let paren_end = find_matching_paren(sql, paren_start)?;

    if !sql[paren_end + 1..].trim().is_empty() {
        return Err(SqlError::syntax(
            "OPTION clause must appear at the end of the query",
        ));
    }

    // Extract the content between parentheses
    let options_content = &sql[paren_start + 1..paren_end];
    let options = parse_option_content(options_content)?;

    // Return SQL without the OPTION clause
    let sql_without_options = sql[..option_idx].trim().to_string();

    Ok((sql_without_options, options))
}

fn find_option_keyword(sql: &str) -> Option<usize> {
    let needle = "OPTION";
    let needle_len = needle.len();

    for (idx, _) in sql.char_indices() {
        let end = idx + needle_len;
        if end > sql.len() {
            break;
        }

        let candidate = &sql[idx..end];
        if !candidate.eq_ignore_ascii_case(needle) {
            continue;
        }

        let prev_is_boundary = idx == 0
            || sql[..idx]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let next_is_boundary =
            end == sql.len() || sql[end..].chars().next().is_some_and(char::is_whitespace);

        if prev_is_boundary && next_is_boundary {
            return Some(idx);
        }
    }

    None
}

fn find_matching_paren(sql: &str, open_idx: usize) -> Result<usize, SqlError> {
    let mut depth = 0usize;

    for (idx, ch) in sql.char_indices().skip_while(|(idx, _)| *idx < open_idx) {
        match ch {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Err(SqlError::syntax(
                        "OPTION clause: unexpected closing parenthesis",
                    ));
                }
                depth -= 1;
                if depth == 0 {
                    return Ok(idx);
                }
            }
            _ => {}
        }
    }

    Err(SqlError::syntax(
        "OPTION clause: missing closing parenthesis",
    ))
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

#[cfg(test)]
#[path = "preprocessor_tests.rs"]
mod tests;
