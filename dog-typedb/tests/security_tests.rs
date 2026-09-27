//! Classification tests only. Actual transaction permissions are tested in live_database.rs.
use dog_typedb::{transactions::analyze_query, TransactionType};

#[test]
fn literal_comment_and_variable_keywords_do_not_escalate_reads() {
    for query in [
        r#"match $p isa person, has name "delete";"#,
        "# define insert delete\nmatch $p isa person; # update\nlimit 2;",
        "match $delete isa person; fetch { 'insert': iid($delete) };",
        r#"match $p isa person, has name "escaped \" insert";"#,
        "match $p isa person, has name 'escaped \\' delete';",
        "match $p isa person, has name 'fetch { define';",
    ] {
        assert!(
            matches!(analyze_query(query).transaction_type, TransactionType::Read),
            "{query}"
        );
    }
}
#[test]
fn actual_write_stages_after_patterns_are_detected() {
    for query in [
        "insert $p isa person;",
        "match $p isa person; delete $p;",
        "# comment\nmatch $p isa person; update $p has name 'Alice';",
        "match { $p isa person; } or { $p isa employee; }; delete $p;",
        "match $p isa person, has name 'delete'; insert $q isa person;",
        "put $p isa person, has name 'Alice';",
    ] {
        assert!(
            matches!(
                analyze_query(query).transaction_type,
                TransactionType::Write
            ),
            "{query}"
        );
    }
}
#[test]
fn schema_routing_ignores_leading_comments_and_function_bodies() {
    for query in [
        "# match insert\ndefine entity person;",
        "undefine entity person;",
        "redefine fun names() -> { string }: match $p isa person, has name $n; return { $n };",
    ] {
        assert!(
            matches!(
                analyze_query(query).transaction_type,
                TransactionType::Schema
            ),
            "{query}"
        );
    }
}
#[test]
fn fetch_whitespace_and_metadata_ignore_literal_values() {
    let analysis =
        analyze_query("match $p isa person; sort $p; offset 1; limit 2; fetch\n{ 'id': iid($p) };");
    assert!(analysis.returns_document_stream && analysis.has_sorting && analysis.has_pagination);
    let analysis =
        analyze_query("match $p isa person, has name 'fetch { sort limit count( let '; ");
    assert!(
        !analysis.returns_document_stream
            && !analysis.has_sorting
            && !analysis.has_pagination
            && !analysis.has_aggregation
            && !analysis.has_functions
    );
}
