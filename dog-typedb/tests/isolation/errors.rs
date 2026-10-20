use super::operation_error;

#[test]
fn failures_keep_private_causes_without_disclosing_them() {
    for (committing, deadline, status) in [
        (false, false, 500),
        (false, true, 408),
        (true, false, 503),
        (true, true, 503),
    ] {
        let error = operation_error(
            anyhow::anyhow!("protected query literal"),
            committing,
            deadline,
        );
        assert_eq!(error.code(), status);
        assert!(error
            .source_ref()
            .unwrap()
            .to_string()
            .contains("protected query literal"));
        let wire = serde_json::to_string(&error.sanitize_for_client()).unwrap();
        assert!(!wire.contains("protected query literal"));
        assert!(!wire.contains("source"));
    }
}

#[test]
fn conflict_text_does_not_turn_an_unknown_commit_into_a_known_rollback() {
    let error = typedb_driver::Error::Other("[STC2] isolation conflict".into());
    let response = operation_error(anyhow::Error::new(error), true, false);
    assert_eq!(response.code(), 503);
    assert!(response.message.contains("unknown"));
}
