use sqlparser::ast::{BinaryOperator, Expr};

use crate::ast::{DistanceMetric, SortDirection};
use crate::parser::parse;

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

    let parsed = super::parse_order_by(&[], None).unwrap();
    assert!(parsed.order_by.is_none());
    assert!(parsed.vector_search.is_none());

    let err =
        parse("SELECT * FROM idx ORDER BY embedding <-> '[0.1]', name ASC LIMIT 10").unwrap_err();
    assert!(err.message.contains("Multiple ORDER BY columns"));
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

    assert!(super::parse_limit(None, None).unwrap().is_none());

    let err = super::parse_limit(
        Some(&Expr::Value(sqlparser::ast::Value::SingleQuotedString(
            "bad".to_string(),
        ))),
        None,
    )
    .unwrap_err();
    assert!(err.message.contains("LIMIT value must be a number"));
}

#[test]
fn test_parse_limit_with_offset() {
    let query = parse("SELECT * FROM idx LIMIT 20 OFFSET 10").unwrap();
    let limit = query.limit.unwrap();
    assert_eq!(limit.count, 20);
    assert_eq!(limit.offset, 10);
}

// Vector search tests
#[test]
fn test_parse_vector_search_basic() {
    let query =
        parse("SELECT * FROM products ORDER BY embedding <-> [0.1, 0.2, 0.3] LIMIT 10").unwrap();
    assert!(query.vector_search.is_some());
    let vs = query.vector_search.unwrap();
    assert_eq!(vs.field, "embedding");
    assert_eq!(vs.vector, "[0.1,0.2,0.3]");
    assert_eq!(vs.k, 10);

    let err = parse("SELECT * FROM products ORDER BY embedding <-> ['oops'] LIMIT 10").unwrap_err();
    assert!(err.message.contains("Vector elements must be numbers"));
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
    let query = parse("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]'").unwrap();
    assert!(query.vector_search.is_some());
    let vs = query.vector_search.unwrap();
    assert_eq!(vs.k, 10);
}

#[test]
fn test_parse_vector_search_l2_distance_metric() {
    // <-> operator should use L2 metric
    let query =
        parse("SELECT * FROM products ORDER BY embedding <-> '[0.1, 0.2, 0.3]' LIMIT 10").unwrap();
    assert!(query.vector_search.is_some());
    let vs = query.vector_search.unwrap();
    assert_eq!(vs.distance_metric, DistanceMetric::L2);
}

#[test]
fn test_parse_vector_search_cosine_distance() {
    // <=> operator should use Cosine metric
    let query =
        parse("SELECT * FROM products ORDER BY embedding <=> '[0.1, 0.2, 0.3]' LIMIT 10").unwrap();
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
        parse("SELECT * FROM products ORDER BY embedding <#> '[0.1, 0.2, 0.3]' LIMIT 10").unwrap();
    assert!(query.vector_search.is_some());
    let vs = query.vector_search.unwrap();
    assert_eq!(vs.field, "embedding");
    assert_eq!(vs.distance_metric, DistanceMetric::InnerProduct);

    assert_eq!(
        super::get_vector_distance_metric(&BinaryOperator::PGCustomBinaryOperator(vec![
            "<=>".to_string(),
        ])),
        Some(DistanceMetric::Cosine)
    );
    assert_eq!(
        super::get_vector_distance_metric(&BinaryOperator::PGCustomBinaryOperator(vec![
            "<#>".to_string(),
        ])),
        Some(DistanceMetric::InnerProduct)
    );
    assert_eq!(
        super::get_vector_distance_metric(&BinaryOperator::PGCustomBinaryOperator(vec![
            "??".to_string(),
        ])),
        None
    );
}
