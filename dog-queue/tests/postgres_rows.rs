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

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn blocked_producers_do_not_starve_completion_on_default_pool() {
    let backend = Arc::new(backend().await);
    let ctx = QueueCtx::new(format!("admission-{}", uuid::Uuid::new_v4()));
    let mut submissions = Vec::new();
    for n in 0..12 {
        let message = JobMessage::new("blocked", vec![1; 65536], "bytes", "blocked")
            .with_idempotency_key(n.to_string());
        let id = backend.enqueue(ctx.clone(), message.clone()).await.unwrap();
        submissions.push((message, id));
    }
    let work = backend
        .enqueue(
            ctx.clone(),
            JobMessage::new("work", vec![], "bytes", "work"),
        )
        .await
        .unwrap();
    let lease = backend
        .dequeue(ctx.clone(), &["work"])
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
    let pid: i32 = tx
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    tx.query(
        "SELECT id FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND queue='blocked' FOR UPDATE",
        &[&ctx.tenant_id],
    )
    .await
    .unwrap();
    let mut producers = tokio::task::JoinSet::new();
    for (message, expected) in submissions {
        let backend = backend.clone();
        let ctx = ctx.clone();
        producers.spawn(async move { (expected, backend.enqueue(ctx, message).await) });
    }
    let observed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            tx.batch_execute("SELECT pg_stat_clear_snapshot()")
                .await
                .unwrap();
            let blocked: i64 = tx
                .query_one(
                    "SELECT count(*) FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid))",
                    &[&pid],
                )
                .await
                .unwrap()
                .get(0);
            if blocked >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let completion = tokio::time::timeout(
        Duration::from_secs(1),
        backend.ack_complete(ctx.clone(), work.clone(), lease.lease_token, None),
    )
    .await;
    // Release locks and drain producers even when the regression fails.
    tx.rollback().await.unwrap();
    while let Some(result) = producers.join_next().await {
        let (expected, actual) = result.unwrap();
        assert_eq!(actual.unwrap(), expected);
    }
    observed.expect("producer queries did not reach the held row locks");
    completion
        .expect("completion was starved behind producers")
        .unwrap();
    assert!(backend.get_status(ctx, work).await.unwrap().is_terminal());
    driver.abort();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn concurrent_completions_isolate_invalid_tokens_and_duplicate_requests() {
    let backend = Arc::new(backend().await);
    let ctx = QueueCtx::new(format!("completion-batch-{}", uuid::Uuid::new_v4()));
    let mut leases = Vec::new();
    for _ in 0..96 {
        backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("batch", vec![7; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        leases.push(backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap());
    }
    let barrier = Arc::new(tokio::sync::Barrier::new(128));
    let mut tasks = tokio::task::JoinSet::new();
    for (n, job) in leases.iter().enumerate() {
        let copies = if n % 3 == 0 { 2 } else { 1 };
        for _ in 0..copies {
            let (backend, ctx, barrier, job) =
                (backend.clone(), ctx.clone(), barrier.clone(), job.clone());
            tasks.spawn(async move {
                barrier.wait().await;
                let token = if n % 3 == 1 {
                    LeaseToken::new()
                } else {
                    job.lease_token
                };
                (
                    n,
                    backend
                        .ack_complete(ctx, job.record.job_id, token, Some(n.to_string()))
                        .await,
                )
            });
        }
    }
    let mut successes = vec![0; 96];
    while let Some(result) = tasks.join_next().await {
        let (n, outcome) = result.unwrap();
        match outcome {
            Ok(()) => successes[n] += 1,
            Err(dog_queue::QueueError::JobAlreadyTerminal) => assert_eq!(n % 3, 0),
            Err(dog_queue::QueueError::InvalidLeaseToken { .. }) => assert_eq!(n % 3, 1),
            Err(e) => panic!("unexpected completion result: {e}"),
        }
    }
    for (n, job) in leases.iter().enumerate() {
        let row = backend
            .get_record(ctx.clone(), job.record.job_id.clone())
            .await
            .unwrap();
        assert_eq!(row.message.payload_bytes, vec![7; 65536]);
        if n % 3 == 1 {
            assert_eq!(successes[n], 0);
            assert!(row.status.is_processing());
            assert_eq!(row.result, None);
        } else {
            assert_eq!(successes[n], 1);
            assert!(matches!(row.status, JobStatus::Completed { .. }));
            assert_eq!(row.result, Some(n.to_string()));
        }
    }
}

#[tokio::test]
async fn completion_batch_size_must_be_bounded() {
    for completion_batch_size in [0, 65] {
        let result = PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: "host=127.0.0.1 port=1".into(),
            },
            tokio_postgres::NoTls,
            PostgresOptions {
                completion_batch_size,
                ..Default::default()
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(dog_queue::QueueError::InvalidConfig(_))
        ));
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn independent_completion_mode_preserves_tokens_and_results() {
    let backend = PostgresBackend::new_with_tls_options(
        PostgresConfig {
            connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        },
        tokio_postgres::NoTls,
        PostgresOptions {
            completion_batch_size: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let ctx = QueueCtx::new(format!("completion-direct-{}", uuid::Uuid::new_v4()));
    let id = backend
        .enqueue(
            ctx.clone(),
            JobMessage::new("direct", vec![1], "bytes", "q"),
        )
        .await
        .unwrap();
    let job = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
    assert!(matches!(
        backend
            .ack_complete(ctx.clone(), id.clone(), LeaseToken::new(), None)
            .await,
        Err(dog_queue::QueueError::InvalidLeaseToken { .. })
    ));
    backend
        .ack_complete(
            ctx.clone(),
            id.clone(),
            job.lease_token.clone(),
            Some("committed".into()),
        )
        .await
        .unwrap();
    assert_eq!(
        backend
            .get_record(ctx.clone(), id.clone())
            .await
            .unwrap()
            .result
            .as_deref(),
        Some("committed")
    );
    assert!(matches!(
        backend.ack_complete(ctx, id, job.lease_token, None).await,
        Err(dog_queue::QueueError::JobAlreadyTerminal)
    ));
}
