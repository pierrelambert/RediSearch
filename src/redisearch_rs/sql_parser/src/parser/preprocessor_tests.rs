use crate::ast::DistanceMetric;
use crate::parser::parse;

#[test]
fn test_parse_simple_select_with_colon_index_name() {
    assert_eq!(
        super::quote_nonstandard_from_identifier("SELECT * FROM idx:all"),
        "SELECT * FROM \"idx:all\""
    );
    assert_eq!(
        super::quote_nonstandard_from_identifier("SELECT * FROM \"idx:all\""),
        "SELECT * FROM \"idx:all\""
    );
    assert_eq!(
        super::quote_nonstandard_from_identifier("SELECT * FROM plain_idx"),
        "SELECT * FROM plain_idx"
    );
    assert_eq!(
        super::quote_nonstandard_from_identifier("SELECT * FROM   "),
        "SELECT * FROM   "
    );

    let sql = "SELECT 'from', \"from\" FROM idx:all";
    let from_idx = super::find_keyword_outside_quotes(sql, "from").unwrap();
    assert_eq!(&sql[from_idx..from_idx + 4], "FROM");

    let query = parse("SELECT * FROM idx:all").unwrap();
    assert_eq!(query.index_name, "idx:all");
    assert!(query.fields.is_empty());
    assert!(query.conditions.is_empty());
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
    let (sql, options) =
        super::extract_option_clause("SELECT * FROM idx OPTION (, text_weight = 0.3)").unwrap();
    assert_eq!(sql, "SELECT * FROM idx");
    assert_eq!(options.vector_weight, None);
    assert_eq!(options.text_weight, Some(0.3));

    let err =
        super::extract_option_clause("SELECT * FROM idx OPTION vector_weight = 0.3").unwrap_err();
    assert!(err.message.contains("followed by parentheses"));

    let err = super::extract_option_clause("SELECT * FROM idx OPTION (vector_weight)").unwrap_err();
    assert!(err.message.contains("Invalid OPTION format"));

    let err =
        super::extract_option_clause("SELECT * FROM idx OPTION (vector_weight = 0.3) trailing")
            .unwrap_err();
    assert!(err.message.contains("must appear at the end"));

    let err = super::extract_option_clause("SELECT * FROM idx OPTION (vector_weight = nope)")
        .unwrap_err();
    assert!(err.message.contains("must be a number"));

    let err = super::find_matching_paren("OPTION (vector_weight = 0.3", 7).unwrap_err();
    assert!(err.message.contains("missing closing parenthesis"));
}

#[test]
fn test_parse_option_clause_after_parenthesized_filter() {
    let query = parse(
        "SELECT * FROM products \
             WHERE (category = 'electronics' OR category = 'hardware') \
             ORDER BY embedding <-> '[0.1, 0.2]' LIMIT 10 \
             OPTION (vector_weight = 0.8, text_weight = 0.2)",
    )
    .unwrap();

    assert!(query.hybrid_search.is_some());
    assert_eq!(query.conditions.len(), 1);
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
