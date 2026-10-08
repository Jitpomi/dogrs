#![cfg(all(feature = "redis", feature = "postgres"))]
use dog_queue::backend::{
    postgres::{PostgresBackend, PostgresConfig},
    redis::{RedisBackend, RedisConfig},
};
use std::{sync::Arc, time::Duration};

mod common;
use common::contract;

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; run with --ignored"]
async fn postgres_cross_connection_contract() {
    let config = PostgresConfig {
        connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
    };
    let a = PostgresBackend::new(config.clone()).await.unwrap();
    let b = PostgresBackend::new(config.clone()).await.unwrap();
    let short = PostgresBackend::new(config)
        .await
        .unwrap()
        .with_lease_duration(Duration::from_millis(50));
    contract(Arc::new(a), Arc::new(b), Arc::new(short)).await;
}

#[tokio::test]
#[ignore = "requires disposable Redis; run with --ignored"]
async fn redis_cross_connection_contract() {
    let config = RedisConfig {
        connection_string: std::env::var("DOGRS_REDIS_URL").unwrap(),
    };
    let a = RedisBackend::new(config.clone()).await.unwrap();
    let b = RedisBackend::new(config.clone()).await.unwrap();
    let short = RedisBackend::new(config)
        .await
        .unwrap()
        .with_lease_duration(Duration::from_millis(50));
    contract(Arc::new(a), Arc::new(b), Arc::new(short)).await;
}

#[cfg(feature = "nats-async")]
#[tokio::test]
#[ignore = "requires disposable NATS JetStream; run with --ignored"]
async fn nats_cross_connection_contract() {
    use dog_queue::backend::nats::{NatsBackend, NatsConfig};
    let config = NatsConfig {
        url: std::env::var("DOGRS_NATS_URL").unwrap(),
        subject: format!("dogrs_test_{}", uuid::Uuid::new_v4().simple()),
    };
    let a = NatsBackend::new(config.clone()).await.unwrap();
    let b = NatsBackend::new(config.clone()).await.unwrap();
    let short = NatsBackend::new(config.clone())
        .await
        .unwrap()
        .with_lease_duration(Duration::from_millis(50));
    contract(Arc::new(a), Arc::new(b), Arc::new(short)).await;
    let js = async_nats::jetstream::new(async_nats::connect(config.url).await.unwrap());
    js.delete_key_value(config.subject).await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with permission to terminate own test session"]
async fn postgres_reconnects_after_connection_loss() {
    let base = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    let name = format!("dogrs-reconnect-{}", uuid::Uuid::new_v4());
    let backend = PostgresBackend::new(PostgresConfig {
        connection_string: format!("{base} application_name={name}"),
    })
    .await
    .unwrap();
    let tenant = dog_queue::QueueCtx::new(name.clone());
    use dog_queue::QueueBackend;
    let id = backend
        .enqueue(
            tenant.clone(),
            dog_queue::JobMessage::new("restart", vec![], "json", "q"),
        )
        .await
        .unwrap();
    let (admin, connection) = tokio_postgres::connect(&base, tokio_postgres::NoTls)
        .await
        .unwrap();
    let task = tokio::spawn(connection);
    admin
        .execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name=$1",
            &[&name],
        )
        .await
        .unwrap();
    let recovered = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if backend.get_status(tenant.clone(), id.clone()).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    recovered.expect("PostgreSQL backend must reconnect without recreating it");
    // Prepared statements are connection-local. A replaced pooled connection
    // must prepare its own enqueue statement instead of reusing the dead one's.
    let message = dog_queue::JobMessage::new("reconnected", vec![7; 65536], "bytes", "q")
        .with_idempotency_key("after-reconnect");
    let next = backend
        .enqueue(tenant.clone(), message.clone())
        .await
        .unwrap();
    assert_eq!(
        backend.enqueue(tenant.clone(), message).await.unwrap(),
        next
    );
    assert_eq!(
        backend
            .get_record(tenant, next)
            .await
            .unwrap()
            .message
            .payload_bytes,
        vec![7; 65536]
    );
    task.abort();
}

#[cfg(feature = "nats-async")]
#[tokio::test]
#[ignore = "requires disposable NATS JetStream"]
async fn nats_capacity_reserves_space_for_completion() {
    use dog_queue::backend::nats::{NatsBackend, NatsConfig};
    use dog_queue::{JobMessage, QueueBackend, QueueCtx};
    let config = NatsConfig {
        url: std::env::var("DOGRS_NATS_URL").unwrap(),
        subject: format!("dogrs_capacity_{}", uuid::Uuid::new_v4().simple()),
    };
    let backend = NatsBackend::new(config.clone()).await.unwrap();
    let tenant = QueueCtx::new("capacity-test");
    // Aggregate tenant payloads exceed the old single-value limit without
    // growing a shared tenant record. Oversized individual jobs still fail.
    for _ in 0..20 {
        backend
            .enqueue(
                tenant.clone(),
                JobMessage::new("large", vec![0; 100_000], "json", "q"),
            )
            .await
            .unwrap();
    }
    assert!(matches!(
        backend
            .enqueue(
                tenant.clone(),
                JobMessage::new("too-large", vec![0; 1_048_576], "bytes", "q")
            )
            .await,
        Err(dog_queue::QueueError::InvalidConfig(_))
    ));
    let job = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert!(backend
        .ack_complete(
            tenant.clone(),
            job.record.job_id.clone(),
            job.lease_token.clone(),
            Some("a".repeat(5000))
        )
        .await
        .is_err());
    backend
        .ack_complete(
            tenant.clone(),
            job.record.job_id,
            job.lease_token,
            Some("a".repeat(4090)),
        )
        .await
        .unwrap();
    assert_eq!(
        backend
            .purge_terminal_before(
                tenant.clone(),
                chrono::Utc::now() + chrono::Duration::seconds(1)
            )
            .await
            .unwrap(),
        1
    );
    backend
        .enqueue(
            tenant,
            JobMessage::new("large", vec![0; 100_000], "json", "q"),
        )
        .await
        .unwrap();
    let js = async_nats::jetstream::new(async_nats::connect(config.url).await.unwrap());
    js.delete_key_value(config.subject).await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with database creation permission"]
async fn postgres_concurrent_first_start_is_safe() {
    let base = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    let database = format!("dogrs_init_{}", uuid::Uuid::new_v4().simple());
    let (admin, connection) = tokio_postgres::connect(&base, tokio_postgres::NoTls)
        .await
        .unwrap();
    let task = tokio::spawn(connection);
    admin
        .batch_execute(&format!("CREATE DATABASE {database}"))
        .await
        .unwrap();
    let config = PostgresConfig {
        connection_string: format!("{base} dbname={database}"),
    };
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let config = config.clone();
        tasks.spawn(async move { PostgresBackend::new(config).await.map(|_| ()) });
    }
    let mut errors = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            error => errors.push(format!("{error:?}")),
        }
    }
    admin
        .batch_execute(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .await
        .unwrap();
    task.abort();
    assert!(
        errors.is_empty(),
        "Concurrent initialization failed: {errors:?}"
    );
}
