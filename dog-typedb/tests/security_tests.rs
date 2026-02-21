#[cfg(test)]
mod security_tests {
    use super::*;
    use dog_typedb::{TypeDBAdapter, transactions::analyze_query};
    use serde_json::json;
    use std::sync::Arc;
    use typedb_driver::TypeDBDriver;

    // Mock TypeDBState for testing
    struct MockTypeDBState {
        driver: Arc<TypeDBDriver>,
        database: String,
    }

    impl dog_typedb::adapter::TypeDBState for MockTypeDBState {
        fn driver(&self) -> &Arc<TypeDBDriver> {
            &self.driver
        }

        fn database(&self) -> &str {
            &self.database
        }
    }

    #[test]
    fn test_query_analysis_detects_delete_operations() {
        let delete_query = "match $u isa user-account, has id \"user-eve\"; delete $u;";
        let analysis = analyze_query(delete_query);
        
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Write));
        assert_eq!(analysis.primary_type.as_str(), "match");
    }

    #[test]
    fn test_query_analysis_detects_insert_operations() {
        let insert_query = "insert $u isa user-account, has id \"new-user\";";
        let analysis = analyze_query(insert_query);
        
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Write));
        assert_eq!(analysis.primary_type.as_str(), "insert");
    }

    #[test]
    fn test_query_analysis_allows_read_operations() {
        let read_query = "match $u isa user-account; limit 10;";
        let analysis = analyze_query(read_query);
        
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Read));
        assert_eq!(analysis.primary_type.as_str(), "match");
    }

    #[test]
    fn test_query_analysis_detects_schema_operations() {
        let schema_query = "define user-account sub entity, has id;";
        let analysis = analyze_query(schema_query);
        
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Schema));
        assert_eq!(analysis.primary_type.as_str(), "define");
    }

    #[tokio::test]
    async fn test_adapter_read_rejects_delete_operations() {
        // This test would require a real TypeDB connection, so we'll test the logic
        // by directly calling the query analysis that the adapter uses
        
        let delete_query_data = json!({
            "query": "match $u isa user-account, has id \"user-eve\"; delete $u;"
        });
        
        let query = delete_query_data.get("query").unwrap().as_str().unwrap();
        let analysis = analyze_query(query);
        
        // Verify that the security check would fail
        assert!(!matches!(analysis.transaction_type, dog_typedb::TransactionType::Read));
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Write));
    }

    #[tokio::test]
    async fn test_adapter_read_rejects_insert_operations() {
        let insert_query_data = json!({
            "query": "insert $u isa user-account, has id \"malicious-user\";"
        });
        
        let query = insert_query_data.get("query").unwrap().as_str().unwrap();
        let analysis = analyze_query(query);
        
        // Verify that the security check would fail
        assert!(!matches!(analysis.transaction_type, dog_typedb::TransactionType::Read));
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Write));
    }

    #[tokio::test]
    async fn test_adapter_read_allows_legitimate_read_operations() {
        let read_query_data = json!({
            "query": "match $u isa user-account; limit 10;"
        });
        
        let query = read_query_data.get("query").unwrap().as_str().unwrap();
        let analysis = analyze_query(query);
        
        // Verify that the security check would pass
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Read));
    }

    #[test]
    fn test_complex_query_with_delete_detected_as_write() {
        let complex_query = r#"
            match 
                $u isa user-account, has id $id;
                $id == "target-user";
            delete $u;
        "#;
        
        let analysis = analyze_query(complex_query);
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Write));
    }

    #[test]
    fn test_fetch_query_detected_as_read() {
        let fetch_query = r#"
            match $u isa user-account, has id $id;
            limit 5;
            fetch {
                "user": { $u.* },
                "id": $id
            };
        "#;
        
        let analysis = analyze_query(fetch_query);
        assert!(matches!(analysis.transaction_type, dog_typedb::TransactionType::Read));
        assert!(analysis.returns_document_stream);
    }
}
