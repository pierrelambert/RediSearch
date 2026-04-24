use crate::ast::{Condition, Value};
use crate::parser::parse;

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
