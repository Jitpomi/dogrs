#![cfg(all(feature = "redis", feature = "nats-async"))]
use dog_queue::{JobMessage, JobStatus, QueueBackend, QueueCtx};
use std::{sync::Arc, time::Duration};

async fn history_and_reuse(a: Arc<dyn QueueBackend>, b: Arc<dyn QueueBackend>) {
    let ctx = QueueCtx::new(format!("records-{}", uuid::Uuid::new_v4()));
    let payload: Vec<u8> = (0..65536).map(|n| (n * 131) as u8).collect();
    let mut completed = Vec::new();
    for n in 0..40 {
        let message = JobMessage::new("history", payload.clone(), "bytes", "q")
            .with_idempotency_key("reusable")
            .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1));
        let (left, right) = tokio::join!(
            a.enqueue(ctx.clone(), message.clone()),
            b.enqueue(ctx.clone(), message)
        );
        let id = left.unwrap();
        assert_eq!(id, right.unwrap());
        assert!(!completed.contains(&id));
        let job = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(job) = b.dequeue(ctx.clone(), &["q"]).await.unwrap() {
                    break job;
                }
                tokio::time::sleep(Duration::from_millis(5)).await
            }
        })
        .await
        .unwrap();
        assert_eq!(job.record.message.payload_bytes, payload);
        a.ack_complete(
            ctx.clone(),
            id.clone(),
            job.lease_token,
            Some(n.to_string()),
        )
        .await
        .unwrap();
        completed.push(id);
    }
    for (n, id) in completed.iter().enumerate() {
        let row = b.get_record(ctx.clone(), id.clone()).await.unwrap();
        assert_eq!(row.message.payload_bytes, payload);
        assert_eq!(row.result, Some(n.to_string()));
        assert!(matches!(row.status, JobStatus::Completed { .. }));
    }
    // Many independent active jobs, each carrying a full payload, cannot share a
    // bounded tenant value. Concurrent claimers must still have unique ownership.
    for n in 0..100 {
        a.enqueue(
            ctx.clone(),
            JobMessage::new("parallel", payload.clone(), "bytes", "q")
                .with_idempotency_key(n.to_string())
                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();
    }
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let backend = b.clone();
        let ctx = ctx.clone();
        tasks.spawn(async move {
            let mut ids = Vec::new();
            while let Some(job) = backend.dequeue(ctx.clone(), &["q"]).await.unwrap() {
                let id = job.record.job_id;
                backend
                    .ack_complete(ctx.clone(), id.clone(), job.lease_token, None)
                    .await
                    .unwrap();
                ids.push(id)
            }
            ids
        });
    }
    let mut claimed = std::collections::HashSet::new();
    while let Some(result) = tasks.join_next().await {
        for id in result.unwrap() {
            assert!(claimed.insert(id));
        }
    }
    assert_eq!(claimed.len(), 100);
    // Selection must stay tenant-scoped and preserve priority/FIFO when the
    // discovery index has entries belonging to several independent tenants.
    let foreign = QueueCtx::new(format!("foreign-{}", uuid::Uuid::new_v4()));
    let make = |priority| {
        JobMessage::new("order", vec![7], "bytes", "q")
            .with_priority(priority)
            .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1))
    };
    a.enqueue(foreign.clone(), make(dog_queue::JobPriority::Critical))
        .await
        .unwrap();
    let mut order = Vec::new();
    for priority in [
        dog_queue::JobPriority::Low,
        dog_queue::JobPriority::Normal,
        dog_queue::JobPriority::High,
        dog_queue::JobPriority::Normal,
    ] {
        order.push(a.enqueue(ctx.clone(), make(priority)).await.unwrap());
        // Redis transition timestamps have millisecond resolution; equal-time
        // records use the job ID tie-breaker rather than insertion order.
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    for position in [2, 1, 3, 0] {
        let job = a.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert_eq!(job.record.job_id, order[position]);
        a.ack_complete(ctx.clone(), job.record.job_id, job.lease_token, None)
            .await
            .unwrap();
    }
    assert!(a.dequeue(ctx.clone(), &["q"]).await.unwrap().is_none());
    let job = a.dequeue(foreign.clone(), &["q"]).await.unwrap().unwrap();
    a.ack_complete(foreign, job.record.job_id, job.lease_token, None)
        .await
        .unwrap();
}
#[tokio::test]
#[ignore = "requires disposable Redis"]
async fn redis_independent_records_and_reused_dedupe() {
    use dog_queue::backend::redis::{RedisBackend, RedisConfig};
    let config = RedisConfig {
        connection_string: std::env::var("DOGRS_REDIS_URL").unwrap(),
    };
    history_and_reuse(
        Arc::new(RedisBackend::new(config.clone()).await.unwrap()),
        Arc::new(RedisBackend::new(config).await.unwrap()),
    )
    .await;
}
#[tokio::test]
#[ignore = "requires disposable JetStream"]
async fn nats_independent_records_and_reused_dedupe() {
    use dog_queue::backend::nats::{NatsBackend, NatsConfig};
    let config = NatsConfig {
        url: std::env::var("DOGRS_NATS_URL").unwrap(),
        subject: format!("records_{}", uuid::Uuid::new_v4().simple()),
    };
    history_and_reuse(
        Arc::new(NatsBackend::new(config.clone()).await.unwrap()),
        Arc::new(NatsBackend::new(config.clone()).await.unwrap()),
    )
    .await;
    let js = async_nats::jetstream::new(async_nats::connect(config.url).await.unwrap());
    js.delete_key_value(config.subject).await.unwrap();
}
