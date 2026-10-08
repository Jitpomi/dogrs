#![cfg(feature = "postgres")]
use dog_queue::{
    backend::postgres::{PostgresBackend, PostgresConfig, PostgresOptions, PostgresPayloadStorage},
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
async fn schema_and_dispatch_bounds_are_validated_before_connecting() {
    for options in [
        PostgresOptions {
            enqueue_batch_size: 0,
            ..Default::default()
        },
        PostgresOptions {
            enqueue_batch_size: 65,
            ..Default::default()
        },
        PostgresOptions {
            schema: Some(String::new()),
            ..Default::default()
        },
        PostgresOptions {
            schema: Some("pg_catalog".into()),
            ..Default::default()
        },
        PostgresOptions {
            schema: Some("bad\0name".into()),
            ..Default::default()
        },
        PostgresOptions {
            schema: Some("a".repeat(64)),
            ..Default::default()
        },
        PostgresOptions {
            batch_concurrency: Some(0),
            ..Default::default()
        },
        PostgresOptions {
            batch_concurrency: Some(5),
            max_connections: 4,
            ..Default::default()
        },
    ] {
        let result = PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: "intentionally invalid connection config".into(),
            },
            tokio_postgres::NoTls,
            options,
        )
        .await;
        assert!(matches!(
            result,
            Err(dog_queue::QueueError::InvalidConfig(_))
        ));
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with CREATE SCHEMA permission"]
async fn payload_storage_preserves_existing_jobs_and_default_reopeners() {
    async fn open(
        uri: &str,
        schema: &str,
        storage: Option<PostgresPayloadStorage>,
    ) -> PostgresBackend {
        PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: uri.into(),
            },
            tokio_postgres::NoTls,
            PostgresOptions {
                schema: Some(schema.into()),
                payload_storage: storage,
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }
    let uri = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    let schema = format!("dogrs_payload_{}", uuid::Uuid::new_v4().simple());
    let original = open(&uri, &schema, Some(PostgresPayloadStorage::Extended)).await;
    let ctx = QueueCtx::new("payload-policy");
    let bytes = vec![42; 65536];
    let message = || JobMessage::new("payload", bytes.clone(), "bytes", "q");
    let old = original.enqueue(ctx.clone(), message()).await.unwrap();
    let (admin, connection) = tokio_postgres::connect(&uri, tokio_postgres::NoTls)
        .await
        .unwrap();
    let connection = tokio::spawn(connection);
    admin
        .query_one(
            "SELECT set_config('search_path',quote_ident($1),false)",
            &[&schema],
        )
        .await
        .unwrap();
    let compressed = |id: JobId| {
        let admin = &admin;
        async move {
            admin.query_one("SELECT pg_column_compression(payload) IS NOT NULL FROM dogrs_queue_jobs_v2 WHERE tenant='payload-policy' AND id=$1", &[&id.as_str()]).await.unwrap().get::<_, bool>(0)
        }
    };
    assert!(
        compressed(old.clone()).await,
        "control payload must actually be compressed"
    );
    let external = open(&uri, &schema, Some(PostgresPayloadStorage::External)).await;
    let fresh = external.enqueue(ctx.clone(), message()).await.unwrap();
    assert!(
        compressed(old.clone()).await,
        "policy changes must not rewrite old values"
    );
    assert!(
        !compressed(fresh.clone()).await,
        "new binary payload must skip compression"
    );
    let reopened = open(&uri, &schema, None).await;
    let policy: String = admin.query_one("SELECT attstorage::text FROM pg_attribute WHERE attrelid='dogrs_queue_jobs_v2'::regclass AND attname='payload'", &[]).await.unwrap().get(0);
    assert_eq!(
        policy, "e",
        "default reconnect must preserve the selected policy"
    );
    admin
        .batch_execute("BEGIN; LOCK TABLE dogrs_queue_jobs_v2 IN ACCESS SHARE MODE")
        .await
        .unwrap();
    let matching = tokio::time::timeout(
        Duration::from_secs(5),
        open(&uri, &schema, Some(PostgresPayloadStorage::External)),
    )
    .await
    .expect("a matching opener must not request an exclusive table lock");
    let blocked_change = PostgresBackend::new_with_tls_options(
        PostgresConfig {
            connection_string: uri.clone(),
        },
        tokio_postgres::NoTls,
        PostgresOptions {
            schema: Some(schema.clone()),
            payload_storage: Some(PostgresPayloadStorage::Extended),
            operation_timeout: Duration::from_millis(200),
            ..Default::default()
        },
    )
    .await;
    assert!(
        blocked_change.is_err(),
        "a blocked policy change must respect the initialization timeout"
    );
    admin.batch_execute("COMMIT").await.unwrap();
    drop(matching);
    for id in [old, fresh] {
        assert_eq!(
            reopened
                .get_record(ctx.clone(), id)
                .await
                .unwrap()
                .message
                .payload_bytes,
            bytes
        );
    }
    for _ in 0..2 {
        let job = reopened
            .dequeue(ctx.clone(), &["q"])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(job.record.message.payload_bytes, bytes);
        reopened
            .ack_complete(ctx.clone(), job.record.job_id, job.lease_token, None)
            .await
            .unwrap();
    }
    let restored = open(&uri, &schema, Some(PostgresPayloadStorage::Extended)).await;
    let id = restored.enqueue(ctx, message()).await.unwrap();
    assert!(
        compressed(id).await,
        "explicitly restoring compression must affect new writes"
    );
    drop((original, external, reopened, restored));
    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
    connection.abort();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL with CREATE SCHEMA permission"]
async fn explicit_schemas_isolate_storage_and_survive_reconnect() {
    async fn scoped(uri: &str, schema: &str) -> PostgresBackend {
        PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: uri.into(),
            },
            tokio_postgres::NoTls,
            PostgresOptions {
                schema: Some(schema.into()),
                batch_concurrency: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }
    let base = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let app = format!("dogrs-schema-{tag}");
    let uri = format!("{base} application_name={app}");
    let names = [format!("dogrs_{tag}_\"a"), format!("dogrs_{tag}_b")];
    // Concurrent creation/opening of one namespace must converge on one store.
    let (a, peer, b) = tokio::join!(
        scoped(&uri, &names[0]),
        scoped(&uri, &names[0]),
        scoped(&uri, &names[1])
    );
    let ctx = QueueCtx::new("same-tenant");
    let message = |byte| {
        JobMessage::new("schema", vec![byte; 65536], "bytes", "q").with_idempotency_key("same-key")
    };
    let id_a = a.enqueue(ctx.clone(), message(1)).await.unwrap();
    let id_b = b.enqueue(ctx.clone(), message(2)).await.unwrap();
    assert_ne!(
        id_a, id_b,
        "idempotency must be scoped to the configured store"
    );
    assert_eq!(
        peer.get_record(ctx.clone(), id_a.clone())
            .await
            .unwrap()
            .message
            .payload_bytes,
        vec![1; 65536]
    );
    assert!(matches!(
        a.get_record(ctx.clone(), id_b.clone()).await,
        Err(dog_queue::QueueError::JobNotFound(_))
    ));
    let job = peer.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
    a.ack_complete(ctx.clone(), id_a.clone(), job.lease_token, None)
        .await
        .unwrap();
    let (admin, connection) = tokio_postgres::connect(&base, tokio_postgres::NoTls)
        .await
        .unwrap();
    let connection = tokio::spawn(connection);
    let stores = admin.query(
        "SELECT c.reltoastrelid FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relname='dogrs_queue_jobs_v2' AND n.nspname=ANY($1)",
        &[&names.as_slice()],
    ).await.unwrap();
    assert_eq!(stores.len(), 2);
    assert_ne!(
        stores[0].get::<_, u32>(0),
        stores[1].get::<_, u32>(0),
        "schemas must own separate physical payload stores"
    );
    admin
        .execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name=$1",
            &[&app],
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let (Ok(left), Ok(right)) = (
                a.get_record(ctx.clone(), id_a.clone()).await,
                b.get_record(ctx.clone(), id_b.clone()).await,
            ) {
                assert!(left.status.is_terminal());
                assert_eq!(left.message.payload_bytes, vec![1; 65536]);
                assert_eq!(right.message.payload_bytes, vec![2; 65536]);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("reconnections must restore the configured storage schema");
    drop((a, peer, b));
    for name in names {
        admin
            .batch_execute(&format!(
                "DROP SCHEMA \"{}\" CASCADE",
                name.replace('"', "\"\"")
            ))
            .await
            .unwrap();
    }
    connection.abort();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn immediate_jobs_use_server_time_without_advancing_explicit_schedules() {
    use tokio_postgres::types::Type;
    let backend = backend().await;
    let tenant = QueueCtx::new(format!("clock-{}", uuid::Uuid::new_v4()));
    let (client, connection) = tokio_postgres::connect(
        &std::env::var("DOGRS_POSTGRES_URL").unwrap(),
        tokio_postgres::NoTls,
    )
    .await
    .unwrap();
    let connection = tokio::spawn(connection);
    let server_now: chrono::DateTime<chrono::Utc> = client
        .query_one("SELECT clock_timestamp()", &[])
        .await
        .unwrap()
        .get(0);
    // Model the envelope produced by a caller whose clock is one hour ahead.
    // Exercise the real admission SQL, not a sleep or a changed database clock.
    let caller_now = server_now + chrono::Duration::hours(1);
    for future in [false, true] {
        let requested = if future {
            // Future on the DB, but past on the simulated fast caller.
            server_now + chrono::Duration::minutes(1)
        } else {
            JobMessage::IMMEDIATE
        };
        let id = JobId::new();
        let payload = vec![17u8; 65536];
        let message = JobMessage::new("clock", vec![], "bytes", "q")
            .with_run_at(requested)
            .with_idempotency_key(id.to_string());
        let mut record = JobRecord::new(id.clone(), &tenant.tenant_id, message.clone());
        record.created_at = caller_now;
        record.updated_at = caller_now;
        let state = serde_json::json!({"record":record,"token":null});
        client
            .query_typed_one(
                include_str!("../src/backend/postgres_enqueue.sql"),
                &[
                    (&tenant.tenant_id, Type::TEXT),
                    (&id.as_str(), Type::TEXT),
                    (&state, Type::JSONB),
                    (&message.queue, Type::TEXT),
                    (&message.job_type, Type::TEXT),
                    (&message.idempotency_key, Type::TEXT),
                    (&i32::from(message.priority.as_u8()), Type::INT4),
                    (&requested, Type::TIMESTAMPTZ),
                    (&payload, Type::BYTEA),
                ],
            )
            .await
            .unwrap();
        let stored = backend
            .get_record(tenant.clone(), id.clone())
            .await
            .unwrap();
        let claim = backend.dequeue(tenant.clone(), &["q"]).await.unwrap();
        if future {
            assert!(
                claim.is_none(),
                "explicit future schedule must be preserved"
            );
            assert_eq!(stored.message.run_at, requested);
        } else {
            let claim =
                claim.expect("caller-due job must be immediately claimable on server clock");
            assert_eq!(claim.record.job_id, id);
            assert_eq!(claim.record.message.payload_bytes, payload);
            assert_eq!(stored.message.run_at, stored.created_at);
            backend
                .ack_complete(tenant.clone(), id, claim.lease_token, None)
                .await
                .unwrap();
        }
    }
    connection.abort();
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
        JobMessage::new("migration", vec![42; 65536], "json", "q"),
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
            payload_storage: Some(PostgresPayloadStorage::External),
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
        vec![42; 65536]
    );
    let compressed: bool = client.query_one("SELECT pg_column_compression(payload) IS NOT NULL FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2", &[&tenant.tenant_id, &id.as_str()]).await.unwrap().get(0);
    assert!(
        !compressed,
        "migration must apply the payload policy before importing rows"
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
    // PostgreSQL assigns child indexes their own names on a partitioned table.
    // Accept only the actual descendants of our active-job indexes, not an
    // arbitrary index that might scan retained terminal history.
    let indexes = client
        .query(
            "WITH RECURSIVE indexes(oid) AS (
               SELECT oid FROM pg_class WHERE oid IN
                 ('dogrs_queue_claim_v2'::regclass,'dogrs_queue_runnable_v2'::regclass)
               UNION ALL
               SELECT i.inhrelid FROM pg_inherits i JOIN indexes p ON i.inhparent=p.oid
             ) SELECT c.relname::text FROM indexes i JOIN pg_class c ON c.oid=i.oid",
            &[],
        )
        .await
        .unwrap();
    assert!(
        indexes.iter().any(|row| {
            let name: String = row.get(0);
            explain.contains(&format!("\"Index Name\":\"{name}\""))
        }),
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
            Some(JobMessage::IMMEDIATE),
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
    blocked_producer_contract(1, 3).await;
    blocked_producer_contract(16, 1).await;
}
async fn blocked_producer_contract(batch_size: usize, blocked_statements: i64) {
    let backend = Arc::new(
        PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: std::env::var("DOGRS_POSTGRES_URL").unwrap(),
            },
            tokio_postgres::NoTls,
            PostgresOptions {
                enqueue_batch_size: batch_size,
                ..Default::default()
            },
        )
        .await
        .unwrap(),
    );
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
            // Independent admissions saturate three producer connections;
            // the default batch dispatcher saturates its one execution slot.
            if blocked >= blocked_statements {
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
async fn batch_sizes_must_be_bounded() {
    for (completion_batch_size, claim_batch_size) in [(0, 16), (65, 16), (64, 0), (64, 65)] {
        let result = PostgresBackend::new_with_tls_options(
            PostgresConfig {
                connection_string: "host=127.0.0.1 port=1".into(),
            },
            tokio_postgres::NoTls,
            PostgresOptions {
                completion_batch_size,
                claim_batch_size,
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
            claim_batch_size: 1,
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

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn batched_claims_preserve_tenants_queues_and_unique_ownership() {
    let backend = Arc::new(backend().await);
    let prefix = uuid::Uuid::new_v4().to_string();
    for tenant in 0..32 {
        for queue in ["a", "b"] {
            backend
                .enqueue(
                    QueueCtx::new(format!("{prefix}-{tenant}")),
                    JobMessage::new("claim", vec![tenant as u8; 65536], "bytes", queue),
                )
                .await
                .unwrap();
        }
    }
    let mut requests = Vec::new();
    for round in 0..4 {
        for tenant in 0..32 {
            let backend = backend.clone();
            let prefix = prefix.clone();
            requests.push(async move {
                let ctx = QueueCtx::new(format!("{prefix}-{tenant}"));
                let queues = match round {
                    0 => vec!["a"],
                    1 => vec!["b"],
                    2 => vec!["a", "b"],
                    _ => vec!["missing"],
                };
                let job = backend.dequeue(ctx.clone(), &queues).await.unwrap();
                if let Some(job) = job {
                    assert!(queues.contains(&job.record.message.queue.as_str()));
                    assert_eq!(job.record.tenant_id, ctx.tenant_id);
                    assert_eq!(job.record.message.payload_bytes, vec![tenant as u8; 65536]);
                    assert_eq!(job.record.attempt, 1);
                    let id = job.record.job_id;
                    backend
                        .ack_complete(ctx, id.clone(), job.lease_token, None)
                        .await
                        .unwrap();
                    Some((tenant, id))
                } else {
                    None
                }
            });
        }
    }
    let mut ids = std::collections::HashSet::new();
    let mut counts = [0; 32];
    for (tenant, id) in futures::future::join_all(requests)
        .await
        .into_iter()
        .flatten()
    {
        assert!(ids.insert(id));
        counts[tenant] += 1;
    }
    assert_eq!(ids.len(), 64);
    assert!(counts.into_iter().all(|count| count == 2));
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn invalid_postgres_text_does_not_poison_other_completions() {
    let backend = backend().await;
    let ctx = QueueCtx::new(format!("nul-{}", uuid::Uuid::new_v4()));
    assert!(matches!(
        backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("bad\0metadata", vec![0], "bytes", "q")
            )
            .await,
        Err(dog_queue::QueueError::InvalidConfig(_))
    ));
    let mut jobs = Vec::new();
    for _ in 0..2 {
        backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("valid", vec![0; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        jobs.push(backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap());
    }
    let (invalid, valid) = tokio::join!(
        backend.ack_complete(
            ctx.clone(),
            jobs[0].record.job_id.clone(),
            jobs[0].lease_token.clone(),
            Some("bad\0result".into())
        ),
        backend.ack_complete(
            ctx.clone(),
            jobs[1].record.job_id.clone(),
            jobs[1].lease_token.clone(),
            Some("good".into())
        ),
    );
    assert!(matches!(
        invalid,
        Err(dog_queue::QueueError::InvalidConfig(_))
    ));
    valid.unwrap();
    assert!(backend
        .get_record(ctx.clone(), jobs[0].record.job_id.clone())
        .await
        .unwrap()
        .status
        .is_processing());
    assert_eq!(
        backend
            .get_record(ctx, jobs[1].record.job_id.clone())
            .await
            .unwrap()
            .result
            .as_deref(),
        Some("good")
    );
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn batched_admissions_preserve_dedupe_payloads_and_individual_validation() {
    let uri = std::env::var("DOGRS_POSTGRES_URL").unwrap();
    for batch_size in [1, 16] {
        let backend = Arc::new(
            PostgresBackend::new_with_tls_options(
                PostgresConfig {
                    connection_string: uri.clone(),
                },
                tokio_postgres::NoTls,
                PostgresOptions {
                    max_connections: 16,
                    enqueue_batch_size: batch_size,
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
        let ctx = QueueCtx::new(format!("admission-batch-{}", uuid::Uuid::new_v4()));
        let barrier = Arc::new(tokio::sync::Barrier::new(129));
        let mut tasks = tokio::task::JoinSet::new();
        for n in 0..128 {
            let backend = backend.clone();
            let ctx = ctx.clone();
            let barrier = barrier.clone();
            tasks.spawn(async move {
                let key = n % 32;
                let message = JobMessage::new("batch", vec![key as u8; 65536], "bytes", "q")
                    .with_idempotency_key(key.to_string());
                barrier.wait().await;
                (key, backend.enqueue(ctx, message).await.unwrap())
            });
        }
        barrier.wait().await;
        assert!(matches!(
            backend
                .enqueue(
                    ctx.clone(),
                    JobMessage::new("bad\0kind", vec![], "bytes", "q")
                )
                .await,
            Err(dog_queue::QueueError::InvalidConfig(_))
        ));
        let mut ids = std::collections::HashMap::new();
        while let Some(result) = tasks.join_next().await {
            let (key, id) = result.unwrap();
            if let Some(previous) = ids.insert(key, id.clone()) {
                assert_eq!(previous, id);
            }
        }
        assert_eq!(ids.len(), 32);
        for (key, id) in ids {
            let record = backend.get_record(ctx.clone(), id).await.unwrap();
            assert_eq!(record.message.payload_bytes, vec![key as u8; 65536]);
            assert_eq!(record.message.idempotency_key, Some(key.to_string()));
        }
    }
}
