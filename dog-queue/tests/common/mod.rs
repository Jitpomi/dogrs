use dog_queue::{JobMessage, JobStatus, QueueBackend, QueueCtx, QueueError};
use std::{sync::Arc, time::Duration};
pub async fn contract(
    a: Arc<dyn QueueBackend>,
    b: Arc<dyn QueueBackend>,
    short: Arc<dyn QueueBackend>,
) {
    let tenant = QueueCtx::new(format!("audit-{}", uuid::Uuid::new_v4()));
    let stranger = QueueCtx::new(format!("stranger-{}", uuid::Uuid::new_v4()));
    let msg = JobMessage::new("test", vec![1, 2, 3], "json", "q")
        .with_idempotency_key("same")
        .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1));
    let (left, right) = tokio::join!(
        a.enqueue(tenant.clone(), msg.clone()),
        b.enqueue(tenant.clone(), msg)
    );
    let id = left.unwrap();
    assert_eq!(id, right.unwrap());
    assert!(matches!(
        b.get_status(stranger.clone(), id.clone()).await,
        Err(QueueError::JobNotFound(_))
    ));
    assert!(b.dequeue(stranger.clone(), &["q"]).await.unwrap().is_none());
    let (left, right) = tokio::join!(
        a.dequeue(tenant.clone(), &["q"]),
        b.dequeue(tenant.clone(), &["q"])
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(
        left.is_some(),
        right.is_some(),
        "Only one process may own a lease"
    );
    let lease = left.or(right).unwrap();
    assert!(a
        .ack_complete(
            stranger.clone(),
            id.clone(),
            lease.lease_token.clone(),
            None
        )
        .await
        .is_err());
    b.ack_complete(
        tenant.clone(),
        id.clone(),
        lease.lease_token,
        Some("42".into()),
    )
    .await
    .unwrap();
    let snapshot = a.get_snapshot(tenant.clone(), id.clone()).await.unwrap();
    let batch = a
        .get_snapshots(tenant.clone(), &[id.clone(), id.clone()])
        .await
        .unwrap();
    assert_eq!(batch.len(), 2);
    assert_eq!(batch[0].job_id, id);
    assert_eq!(batch[1].job_id, id);
    assert!(a
        .get_snapshots(stranger.clone(), std::slice::from_ref(&id))
        .await
        .is_err());
    let record = a.get_record(tenant.clone(), id).await.unwrap();
    assert_eq!(snapshot.attempt, record.attempt);
    assert_eq!(snapshot.result, record.result);
    assert!(serde_json::to_value(snapshot)
        .unwrap()
        .get("message")
        .is_none());
    assert!(matches!(record.status, JobStatus::Completed { .. }));
    assert_eq!(record.result.as_deref(), Some("42"));
    assert_eq!(record.message.payload_bytes, vec![1, 2, 3]);

    let cancel = a
        .enqueue(
            tenant.clone(),
            JobMessage::new("cancel", vec![], "json", "q")
                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();
    let lease = b.dequeue(tenant.clone(), &["q"]).await.unwrap().unwrap();
    a.cancel(tenant.clone(), cancel.clone()).await.unwrap();
    assert!(matches!(
        b.ack_complete(tenant.clone(), cancel, lease.lease_token, None)
            .await,
        Err(QueueError::JobCanceled)
    ));

    let retry = a
        .enqueue(
            tenant.clone(),
            JobMessage::new("retry", vec![], "json", "q")
                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();
    let lease = b.dequeue(tenant.clone(), &["q"]).await.unwrap().unwrap();
    b.ack_fail(
        tenant.clone(),
        retry.clone(),
        lease.lease_token,
        "retry me".into(),
        Some(chrono::Utc::now() - chrono::Duration::seconds(1)),
    )
    .await
    .unwrap();
    let lease = a.dequeue(tenant.clone(), &["q"]).await.unwrap().unwrap();
    assert_eq!(lease.record.attempt, 2);
    a.ack_complete(tenant.clone(), retry, lease.lease_token, None)
        .await
        .unwrap();

    let expired = a
        .enqueue(
            tenant.clone(),
            JobMessage::new("expire", vec![], "json", "q")
                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();
    let lease = short
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(matches!(
        b.ack_complete(tenant.clone(), expired.clone(), lease.lease_token, None)
            .await,
        Err(QueueError::LeaseExpired)
    ));
    let reclaimed = b.reclaim_expired_leases().await.unwrap();
    assert!(reclaimed.iter().any(|r| r.job_id == expired));
    assert!(matches!(
        a.get_status(tenant, expired).await.unwrap(),
        JobStatus::Retrying { .. }
    ));
}
