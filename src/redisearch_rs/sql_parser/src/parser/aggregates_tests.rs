use sqlparser::ast::{Expr, SelectItem, SetExpr, Statement};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use crate::ast::{AggregateFunction, Condition};
use crate::parser::parse;

fn parsed_select(sql: &str) -> sqlparser::ast::Select {
    let dialect = PostgreSqlDialect {};
    let mut statements = Parser::parse_sql(&dialect, sql).unwrap();
    let Statement::Query(query) = statements.remove(0) else {
        panic!("Expected SELECT query");
    };
    let SetExpr::Select(select) = *query.body else {
        panic!("Expected SELECT body");
    };
    *select
}

fn parsed_function(sql: &str) -> sqlparser::ast::Function {
    let select = parsed_select(sql);
    match &select.projection[0] {
        SelectItem::UnnamedExpr(Expr::Function(func)) => func.clone(),
        other => panic!("Expected function projection, got {other:?}"),
    }
}

fn parsed_having(sql: &str) -> Expr {
    parsed_select(sql)
        .having
        .expect("Expected HAVING expression")
}

#[test]
fn test_parse_count_star() {
    let query = parse("SELECT COUNT(*) FROM idx").unwrap();
    assert!(query.fields.is_empty());
    assert_eq!(query.aggregates.len(), 1);
    assert!(matches!(
        query.aggregates[0].function,
        AggregateFunction::Count
    ));
    assert!(query.aggregates[0].field.is_none()); // COUNT(*) has no field

    let query = parse("SELECT SUM(idx.price) FROM idx").unwrap();
    assert!(matches!(
        query.aggregates[0].function,
        AggregateFunction::Sum
    ));
    assert_eq!(query.aggregates[0].field.as_deref(), Some("price"));

    let agg = super::parse_aggregate_function(
        &parsed_function("SELECT COUNT(category) FROM idx"),
        Some("cnt".to_string()),
    )
    .unwrap()
    .unwrap();
    assert!(matches!(agg.function, AggregateFunction::Count));
    assert_eq!(agg.field.as_deref(), Some("category"));
    assert_eq!(agg.alias.as_deref(), Some("cnt"));

    let query = parse("SELECT AVG(price), MIN(price), MAX(price) FROM idx").unwrap();
    assert!(matches!(
        query.aggregates[0].function,
        AggregateFunction::Avg
    ));
    assert!(matches!(
        query.aggregates[1].function,
        AggregateFunction::Min
    ));
    assert!(matches!(
        query.aggregates[2].function,
        AggregateFunction::Max
    ));

    let query =
        parse("SELECT COUNT_DISTINCT(user_id), COUNT_DISTINCTISH(user_id), STDDEV(price) FROM idx")
            .unwrap();
    assert!(matches!(
        query.aggregates[0].function,
        AggregateFunction::CountDistinct
    ));
    assert!(matches!(
        query.aggregates[1].function,
        AggregateFunction::CountDistinctish
    ));
    assert!(matches!(
        query.aggregates[2].function,
        AggregateFunction::Stddev
    ));

    let query = parse("SELECT TOLIST(name), HLL(user_id), HLL_SUM(hll_blob) FROM idx").unwrap();
    assert!(matches!(
        query.aggregates[0].function,
        AggregateFunction::Tolist
    ));
    assert!(matches!(
        query.aggregates[1].function,
        AggregateFunction::Hll
    ));
    assert!(matches!(
        query.aggregates[2].function,
        AggregateFunction::HllSum
    ));
}

#[test]
fn test_parse_aggregate_sum() {
    let query = parse("SELECT QUANTILE(price, 0.5) FROM idx").unwrap();
    match &query.aggregates[0].function {
        AggregateFunction::Quantile { percentile } => {
            assert!((*percentile - 0.5).abs() < f64::EPSILON);
        }
        other => panic!("Expected Quantile aggregate, got {other:?}"),
    }

    let err = parse("SELECT QUANTILE(price) FROM idx").unwrap_err();
    assert!(
        err.message
            .contains("QUANTILE requires exactly 2 arguments")
    );

    let err = parse("SELECT QUANTILE(price, 1.5) FROM idx").unwrap_err();
    assert!(err.message.contains("between 0.0 and 1.0"));

    let err = super::parse_aggregate_function(
        &parsed_function("SELECT QUANTILE(price, 'oops') FROM idx"),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("must be a number"));

    let query = parse("SELECT RANDOM_SAMPLE(name, 5) FROM idx").unwrap();
    match &query.aggregates[0].function {
        AggregateFunction::RandomSample { size } => assert_eq!(*size, 5),
        other => panic!("Expected RandomSample aggregate, got {other:?}"),
    }

    let err = super::parse_aggregate_function(
        &parsed_function("SELECT RANDOM_SAMPLE(name) FROM idx"),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("requires exactly 2 arguments"));

    let err = super::parse_aggregate_function(
        &parsed_function("SELECT RANDOM_SAMPLE(name, 'oops') FROM idx"),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("positive integer"));

    let err = parse("SELECT RANDOM_SAMPLE(name, 0) FROM idx").unwrap_err();
    assert!(err.message.contains("between 1 and 1000"));

    let query = parse("SELECT FIRST_VALUE(name, price, 'ASC') FROM idx").unwrap();
    match &query.aggregates[0].function {
        AggregateFunction::FirstValue {
            sort_field,
            ascending,
        } => {
            assert_eq!(sort_field, "price");
            assert!(*ascending);
        }
        other => panic!("Expected FirstValue aggregate, got {other:?}"),
    }

    let query = parse("SELECT FIRST_VALUE(name, price) FROM idx").unwrap();
    match &query.aggregates[0].function {
        AggregateFunction::FirstValue { ascending, .. } => assert!(!ascending),
        other => panic!("Expected FirstValue aggregate, got {other:?}"),
    }

    let err = super::parse_aggregate_function(
        &parsed_function("SELECT FIRST_VALUE(name) FROM idx"),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("requires 2-3 arguments"));

    let err = parse("SELECT FIRST_VALUE(name, price, 'SIDEWAYS') FROM idx").unwrap_err();
    assert!(err.message.contains("must be 'ASC' or 'DESC'"));
}

#[test]
fn test_parse_group_by() {
    let query = parse(
        "SELECT category, COUNT(*) AS cnt, SUM(price) AS total \
         FROM idx GROUP BY category HAVING (COUNT(*) = 5 OR SUM(price) != 7)",
    )
    .unwrap();
    assert!(query.group_by.is_some());
    let group_by = query.group_by.unwrap();
    assert_eq!(group_by.fields, vec!["category"]);

    match query.having.unwrap() {
        Condition::Or(left, right) => {
            assert!(matches!(*left, Condition::Equals { .. }));
            assert!(matches!(*right, Condition::NotEquals { .. }));
        }
        other => panic!("Expected OR HAVING condition, got {other:?}"),
    }

    let query = parse(
        "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price \
         FROM idx GROUP BY category HAVING COUNT(*) > 2 AND AVG(price) < 5",
    )
    .unwrap();
    assert!(matches!(query.having.unwrap(), Condition::And(_, _)));

    let condition = super::parse_having_expression(&parsed_having(
        "SELECT category, COUNT(*) AS cnt, SUM(price) AS total \
         FROM idx GROUP BY category HAVING (COUNT(*) = 5 OR SUM(price) != 7)",
    ))
    .unwrap();
    assert!(matches!(condition, Condition::Or(_, _)));

    let condition = super::parse_having_expression(&parsed_having(
        "SELECT category, COUNT(*) AS cnt, AVG(price) AS avg_price \
         FROM idx GROUP BY category HAVING COUNT(*) > 2 AND AVG(price) < 5",
    ))
    .unwrap();
    assert!(matches!(condition, Condition::And(_, _)));

    assert_eq!(
        super::extract_having_field(&Expr::Identifier(sqlparser::ast::Ident::new("cnt"))).unwrap(),
        "cnt"
    );
    assert_eq!(
        super::extract_having_field(&Expr::Function(parsed_function("SELECT COUNT(*) FROM idx")))
            .unwrap(),
        "count"
    );
    assert_eq!(
        super::extract_having_field(&Expr::Function(parsed_function(
            "SELECT SUM(price) FROM idx"
        )))
        .unwrap(),
        "sum_price"
    );
    assert_eq!(
        super::extract_having_field(&Expr::Function(parsed_function(
            "SELECT FIRST_VALUE(name, price) FROM idx",
        )))
        .unwrap(),
        "first_value"
    );

    let err = super::parse_having_expression(&Expr::Function(parsed_function(
        "SELECT COUNT(*) FROM idx",
    )))
    .unwrap_err();
    assert!(err.message.contains("Only simple comparisons"));

    let err = parse("SELECT category FROM idx GROUP BY ALL").unwrap_err();
    assert!(err.message.contains("GROUP BY ALL"));

    let err = parse("SELECT category, COUNT(*) FROM idx GROUP BY category HAVING COUNT(*) + 1")
        .unwrap_err();
    assert!(err.message.contains("not supported in HAVING clause"));
}
