use super::operation_error;

#[test]
fn typed_policy_denial_is_forbidden_but_never_certifies_a_commit_rollback() {
    for (committing, deadline, status) in [
        (false, false, 403),
        (false, true, 408),
        (true, false, 503),
        (true, true, 503),
    ] {
        let error = anyhow::Error::new(super::PolicyDenied).context("protected schema detail");
        let response = operation_error(error, committing, deadline);
        assert_eq!(response.code(), status);
        let wire = serde_json::to_string(&response.sanitize_for_client()).unwrap();
        assert!(!wire.contains("protected schema detail"));
    }
    let spoof = operation_error(
        anyhow::anyhow!("Schema policy does not authorize this operation"),
        false,
        false,
    );
    assert_eq!(spoof.code(), 500);
}

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
