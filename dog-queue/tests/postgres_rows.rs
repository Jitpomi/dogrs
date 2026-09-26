#![cfg(feature = "postgres")]
use dog_queue::{
    backend::postgres::{PostgresBackend, PostgresConfig, PostgresOptions},
    JobId, JobMessage, JobRecord, JobStatus, LeaseToken, QueueBackend, QueueCtx,
};
use std::{sync::Arc, time::Duration};

async fn backend() -> PostgresBackend {
    PostgresBackend::new(PostgresConfig {
        connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
    })
    .await
    .unwrap()
}

async fn batched(options: dog_queue::backend::postgres::PostgresBatchOptions) -> PostgresBackend {
    PostgresBackend::new_with_tls_options(
        PostgresConfig {
            connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        },
        tokio_postgres::NoTls,
        PostgresOptions {
            max_connections: 8,
            enqueue_batch: Some(options),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn batched_acknowledgements_are_committed_and_tenant_dedupe_is_preserved() {
    use dog_queue::backend::postgres::PostgresBatchOptions;
    let writer = Arc::new(
        batched(PostgresBatchOptions {
            max_jobs: 16,
            max_delay: Duration::from_millis(10),
            ..Default::default()
        })
        .await,
    );
    let reader = Arc::new(backend().await);
    let prefix = format!("batch-{}", uuid::Uuid::new_v4());
    let mut tasks = tokio::task::JoinSet::new();
    for tenant in 0..8 {
        for n in 0..32 {
            let writer = writer.clone();
            let reader = reader.clone();
            let name = format!("{prefix}-{tenant}");
            tasks.spawn(async move {
                let key = n / 2;
                let payload = vec![key as u8; 65536];
                let id = writer
                    .enqueue(
                        QueueCtx::new(&name),
                        JobMessage::new("batch", payload.clone(), "bytes", "q")
                            .with_idempotency_key(key.to_string()),
                    )
                    .await
                    .unwrap();
                // Another connection must see the complete payload immediately
                // after acknowledgement, not merely after the batch drains.
                let row = reader
                    .get_record(QueueCtx::new(&name), id.clone())
                    .await
                    .unwrap();
                assert_eq!(row.tenant_id, name);
                assert_eq!(row.message.payload_bytes, payload);
                (name, key, id)
            });
        }
    }
    let mut unique = std::collections::HashMap::new();
    while let Some(result) = tasks.join_next().await {
        let (name, key, id) = result.unwrap();
        if let Some(old) = unique.insert((name, key), id.clone()) {
            assert_eq!(old, id);
        }
    }
    assert_eq!(unique.len(), 128);
    drop(writer);
    for ((tenant, _), id) in unique {
        assert!(reader.get_record(QueueCtx::new(tenant), id).await.is_ok());
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn failed_batch_rolls_back_every_job_and_can_accept_the_next_batch() {
    use dog_queue::backend::postgres::PostgresBatchOptions;
    let writer = batched(PostgresBatchOptions {
        workers: 1,
        max_jobs: 2,
        max_delay: Duration::from_millis(100),
    })
    .await;
    let uri = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    let (client, connection) = tokio_postgres::connect(&uri, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let constraint = format!("batch_reject_{suffix}");
    let bad_kind = format!("rejected_{suffix}");
    let tenant = format!("rollback-{suffix}");
    // Unique test-only predicate; NOT VALID avoids scanning unrelated fixtures.
    client.batch_execute(&format!("ALTER TABLE dogrs_queue_jobs_v2 ADD CONSTRAINT {constraint} CHECK(kind <> '{bad_kind}') NOT VALID")).await.unwrap();
    let (good, bad) = tokio::join!(
        writer.enqueue(
            QueueCtx::new(&tenant),
            JobMessage::new("good", vec![1; 65536], "bytes", "q")
        ),
        writer.enqueue(
            QueueCtx::new(&tenant),
            JobMessage::new(bad_kind, vec![2; 65536], "bytes", "q")
        ),
    );
    let count: i64 = client
        .query_one(
            "SELECT count(*) FROM dogrs_queue_jobs_v2 WHERE tenant=$1",
            &[&tenant],
        )
        .await
        .unwrap()
        .get(0);
    client
        .batch_execute(&format!(
            "ALTER TABLE dogrs_queue_jobs_v2 DROP CONSTRAINT {constraint}"
        ))
        .await
        .unwrap();
    assert!(
        good.is_err() && bad.is_err(),
        "no member of a failed transaction may be acknowledged"
    );
    assert_eq!(count, 0);
    let id = writer
        .enqueue(
            QueueCtx::new(&tenant),
            JobMessage::new("after", vec![3; 65536], "bytes", "q"),
        )
        .await
        .unwrap();
    assert_eq!(
        writer
            .get_record(QueueCtx::new(tenant), id)
            .await
            .unwrap()
            .message
            .payload_bytes,
        vec![3; 65536]
    );
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn parallel_claims_do_not_serialize_unrelated_jobs_or_duplicate_owners() {
    let backend = Arc::new(backend().await);
    let tenant = QueueCtx::new(format!("rows-{}", uuid::Uuid::new_v4()));
    for _ in 0..32 {
        backend
            .enqueue(
                tenant.clone(),
                JobMessage::new("parallel", vec![], "json", "q"),
            )
            .await
            .unwrap();
    }
    let mut workers = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let backend = backend.clone();
        let tenant = tenant.clone();
        workers.spawn(async move {
            let mut ids = vec![];
            while let Some(job) = backend.dequeue(tenant.clone(), &["q"]).await.unwrap() {
                ids.push(job.record.job_id.clone());
                backend
                    .ack_complete(tenant.clone(), job.record.job_id, job.lease_token, None)
                    .await
                    .unwrap();
            }
            ids
        });
    }
    let mut ids = vec![];
    while let Some(result) = workers.join_next().await {
        ids.extend(result.unwrap());
    }
    assert_eq!(ids.len(), 32);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 32);
    assert_eq!(
        backend
            .purge_terminal_before(tenant, chrono::Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap(),
        32
    );
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn locked_job_does_not_block_another_claim() {
    let backend = backend().await;
    let tenant = QueueCtx::new(format!("skip-{}", uuid::Uuid::new_v4()));
    let first = backend
        .enqueue(tenant.clone(), JobMessage::new("lock", vec![], "json", "q"))
        .await
        .unwrap();
    let second = backend
        .enqueue(tenant.clone(), JobMessage::new("lock", vec![], "json", "q"))
        .await
        .unwrap();
    let (mut client, connection) = tokio_postgres::connect(
        &std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    let task = tokio::spawn(connection);
    let tx = client.transaction().await.unwrap();
    tx.query_one(
        "SELECT id FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2 FOR UPDATE",
        &[&tenant.tenant_id, &first.as_str()],
    )
    .await
    .unwrap();
    let leased = tokio::time::timeout(Duration::from_secs(2), backend.dequeue(tenant, &["q"]))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(leased.record.job_id, second);
    tx.rollback().await.unwrap();
    task.abort();
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL with CREATE DATABASE permission"]
async fn migration_preserves_jobs_and_fences_legacy_writers() {
    let base: tokio_postgres::Config = std::env::var("DOGRS_POSTGRES_URL")
        .unwrap()
        .parse()
        .unwrap();
    let (admin, connection) = base.connect(tokio_postgres::NoTls).await.unwrap();
    let admin_task = tokio::spawn(connection);
    let database = format!("dogrs_migration_{}", uuid::Uuid::new_v4().simple());
    admin
        .batch_execute(&format!("CREATE DATABASE {database}"))
        .await
        .unwrap();
    let mut config = base;
    config.dbname(&database);
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    let task = tokio::spawn(connection);
    let tenant = QueueCtx::new("migration");
    let id = JobId::new();
    let token = LeaseToken::new();
    let mut record = JobRecord::new(
        id.clone(),
        "migration",
        JobMessage::new("migration", vec![42], "json", "q"),
    );
    record.attempt = 1;
    record.start_processing(
        token.clone(),
        chrono::Utc::now() + chrono::Duration::minutes(5),
    );
    let state = serde_json::json!({"jobs":{id.as_str():{"record":record,"token":token}}});
    client
        .batch_execute(
            "CREATE TABLE dogrs_queue_state_v1(tenant TEXT PRIMARY KEY,state JSONB NOT NULL)",
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO dogrs_queue_state_v1 VALUES($1,$2)",
            &[&tenant.tenant_id, &state],
        )
        .await
        .unwrap();
    // Preserve URL credentials without logging them; database is an owned random name.
    let uri = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    // Tests pass keyword config, so appending dbname overrides the test base.
    let cfg = PostgresConfig {
        connection_string: format!("{uri} dbname={database}"),
    };
    assert!(PostgresBackend::new(cfg.clone()).await.is_err());
    let backend = PostgresBackend::new_with_tls_options(
        cfg.clone(),
        tokio_postgres::NoTls,
        PostgresOptions {
            migrate_legacy: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(
        backend
            .get_record(tenant.clone(), id.clone())
            .await
            .unwrap()
            .message
            .payload_bytes,
        vec![42]
    );
    backend
        .ack_complete(tenant.clone(), id.clone(), token, None)
        .await
        .unwrap();
    assert!(client
        .execute("UPDATE dogrs_queue_state_v1 SET state=state", &[])
        .await
        .is_err());
    drop(backend);
    let reopened = PostgresBackend::new(cfg).await.unwrap();
    assert!(matches!(
        reopened.get_status(tenant, id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
    drop(reopened);
    drop(client);
    task.abort();
    admin
        .batch_execute(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .await
        .unwrap();
    admin_task.abort();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn retained_history_does_not_enter_the_claim_scan() {
    let backend = backend().await;
    let tenant = QueueCtx::new(format!("history-{}", uuid::Uuid::new_v4()));
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("history", vec![1], "json", "q"),
        )
        .await
        .unwrap();
    let lease = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    backend
        .ack_complete(tenant.clone(), id.clone(), lease.lease_token, None)
        .await
        .unwrap();
    let (client, connection) = tokio_postgres::connect(
        &std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    let task = tokio::spawn(connection);
    client.execute("INSERT INTO dogrs_queue_jobs_v2 (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload) SELECT tenant,'history-'||n,jsonb_set(state,'{record,job_id}',to_jsonb('history-'||n)),queue,kind,NULL,priority,created_at,updated_at,eligible_at,NULL,status,false,payload FROM dogrs_queue_jobs_v2 CROSS JOIN generate_series(1,10000) n WHERE tenant=$1 AND id=$2", &[&tenant.tenant_id,&id.as_str()]).await.unwrap();
    let fresh = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("fresh", vec![2], "json", "q"),
        )
        .await
        .unwrap();
    client
        .batch_execute("ANALYZE dogrs_queue_jobs_v2")
        .await
        .unwrap();
    let plan: serde_json::Value=client.query_one("EXPLAIN (ANALYZE, FORMAT JSON) SELECT id FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND queue=ANY($2) AND eligible_at <= statement_timestamp() AND status IN ('enqueued','retrying') ORDER BY priority DESC,created_at,id LIMIT 1 FOR UPDATE SKIP LOCKED", &[&tenant.tenant_id,&vec!["q"]]).await.unwrap().get(0);
    let explain = plan.to_string();
    assert!(
        explain.contains("dogrs_queue_claim_v2") || explain.contains("dogrs_queue_runnable_v2"),
        "claim must use an active-job index: {explain}"
    );
    assert_eq!(
        backend
            .dequeue(tenant.clone(), &["q"])
            .await
            .unwrap()
            .unwrap()
            .record
            .job_id,
        fresh
    );
    assert_eq!(
        backend
            .purge_terminal_before(tenant, chrono::Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap(),
        10001
    );
    task.abort();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn binary_payload_survives_heartbeat_retry_and_completion() {
    let backend = backend().await;
    let tenant = QueueCtx::new(format!("payload-{}", uuid::Uuid::new_v4()));
    let bytes: Vec<u8> = (0..=255).cycle().take(65536).collect();
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("binary", bytes.clone(), "opaque", "q"),
        )
        .await
        .unwrap();
    let first = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.record.message.payload_bytes, bytes);
    backend
        .heartbeat_extend(
            tenant.clone(),
            id.clone(),
            first.lease_token.clone(),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    backend
        .ack_fail(
            tenant.clone(),
            id.clone(),
            first.lease_token,
            "retry".into(),
            Some(chrono::Utc::now()),
        )
        .await
        .unwrap();
    let retry = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.record.message.payload_bytes, bytes);
    backend
        .ack_complete(tenant.clone(), id.clone(), retry.lease_token, None)
        .await
        .unwrap();
    assert_eq!(
        backend
            .get_record(tenant, id)
            .await
            .unwrap()
            .message
            .payload_bytes,
        bytes
    );
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn opening_pools_does_not_lock_out_running_jobs() {
    let active = Arc::new(backend().await);
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let active = active.clone();
        tasks.spawn(async move {
            let _another = backend().await;
            let ctx = QueueCtx::new(format!("startup-{}", uuid::Uuid::new_v4()));
            let id = active
                .enqueue(ctx.clone(), JobMessage::new("startup", vec![], "json", "q"))
                .await
                .unwrap();
            let job = active.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
            active
                .ack_complete(ctx, id, job.lease_token, None)
                .await
                .unwrap();
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn completion_waiting_on_row_lock_cannot_cross_lease_deadline() {
    let backend = Arc::new(
        backend()
            .await
            .with_lease_duration(Duration::from_millis(200)),
    );
    let tenant = QueueCtx::new(format!("commit-fence-{}", uuid::Uuid::new_v4()));
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("fence", vec![7; 65536], "bytes", "q")
                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();
    let job = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    let (mut client, connection) = tokio_postgres::connect(
        &std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    let driver = tokio::spawn(connection);
    let tx = client.transaction().await.unwrap();
    tx.query_one(
        "SELECT id FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2 FOR UPDATE",
        &[&tenant.tenant_id, &id.as_str()],
    )
    .await
    .unwrap();
    let worker = backend.clone();
    let ctx = tenant.clone();
    let key = id.clone();
    let completion =
        tokio::spawn(async move { worker.ack_complete(ctx, key, job.lease_token, None).await });
    tokio::time::sleep(Duration::from_millis(350)).await;
    tx.commit().await.unwrap();
    assert!(matches!(
        completion.await.unwrap(),
        Err(dog_queue::QueueError::LeaseExpired)
    ));
    assert!(matches!(
        backend.get_status(tenant, id).await.unwrap(),
        JobStatus::Processing { .. }
    ));
    driver.abort();
}
