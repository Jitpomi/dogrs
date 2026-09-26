//! Controlled faults against a disposable local PostgreSQL server, never a provider failover claim.
#![cfg(feature = "postgres")]
use dog_queue::{
    backend::postgres::{PostgresBackend, PostgresConfig},
    JobMessage, JobStatus, QueueBackend, QueueCtx,
};
use std::time::Duration;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
};

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; DOGRS_PARTITION_SECONDS=300 exercises a five-minute outage"]
async fn postgres_recovers_after_partition_and_fences_expired_owner() {
    let config: tokio_postgres::Config = std::env::var("DOGRS_POSTGRES_URL")
        .unwrap()
        .parse()
        .unwrap();
    // The test is intentionally restricted to local infrastructure under our control.
    assert!(
        matches!(config.get_hosts(), [tokio_postgres::config::Host::Tcp(host)] if host == "127.0.0.1")
    );
    let upstream = ("127.0.0.1", config.get_ports()[0]);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (enabled, mut state) = watch::channel(true);
    let proxy = tokio::spawn(async move {
        loop {
            let (mut downstream, _) = listener.accept().await.unwrap();
            if !*state.borrow_and_update() {
                continue;
            }
            let mut state = state.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = async {
                        let Ok(mut remote) = TcpStream::connect(upstream).await else { return; };
                        let _ = tokio::io::copy_bidirectional(&mut downstream, &mut remote).await;
                    } => {}
                    _ = state.changed() => {}
                }
            });
        }
    });
    // Preserve credentials privately, replacing only the local endpoint.
    let user = config.get_user().unwrap_or("postgres");
    let password = std::str::from_utf8(config.get_password().unwrap_or_default()).unwrap();
    let database = config.get_dbname().unwrap_or("postgres");
    assert!(
        !user.contains(['\'', '\\'])
            && !password.contains(['\'', '\\'])
            && !database.contains(['\'', '\\'])
    );
    let backend = PostgresBackend::new(PostgresConfig {
        connection_string: format!("host=127.0.0.1 port={port} user='{user}' password='{password}' dbname='{database}' connect_timeout=2"),
    }).await.unwrap().with_lease_duration(Duration::from_secs(1));
    let tenant = QueueCtx::new(format!("dogrs-test-partition-{}", uuid::Uuid::new_v4()));
    let id = backend
        .enqueue(
            tenant.clone(),
            JobMessage::new("partition", vec![1], "json", "q"),
        )
        .await
        .unwrap();
    let original = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    enabled.send(false).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let offline = tokio::time::timeout(
        Duration::from_secs(5),
        backend.get_status(tenant.clone(), id.clone()),
    )
    .await;
    assert!(
        matches!(offline, Ok(Err(_))),
        "connection failure must surface within deadline"
    );
    let seconds: u64 = std::env::var("DOGRS_PARTITION_SECONDS")
        .unwrap_or_else(|_| "2".into())
        .parse()
        .unwrap();
    assert!((2..=300).contains(&seconds));
    println!("PARTITION_STARTED seconds={seconds}");
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    enabled.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if backend.get_status(tenant.clone(), id.clone()).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("same backend instance must reconnect after the partition");
    assert!(backend
        .ack_complete(tenant.clone(), id.clone(), original.lease_token, None)
        .await
        .is_err());
    backend.reclaim_expired_leases().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let recovered = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.record.job_id, id);
    assert_eq!(recovered.record.attempt, 2);
    backend
        .ack_complete(tenant.clone(), id.clone(), recovered.lease_token, None)
        .await
        .unwrap();
    assert!(matches!(
        backend.get_status(tenant.clone(), id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
    backend
        .purge_terminal_before(tenant, chrono::Utc::now() + chrono::Duration::seconds(1))
        .await
        .unwrap();
    println!("PARTITION_RECOVERY_VERIFIED seconds={seconds} stale_owner_rejected=true");
    drop(backend);
    proxy.abort();
}

#[tokio::test]
#[ignore = "requires a restored snapshot of the partition fixture in an isolated database"]
async fn postgres_restored_snapshot_reclaims_inflight_job() {
    let uri = std::env::var("DOGRS_RESTORED_POSTGRES_URL").unwrap();
    let config: tokio_postgres::Config = uri.parse().unwrap();
    assert!(
        matches!(config.get_hosts(), [tokio_postgres::config::Host::Tcp(host)] if host == "127.0.0.1")
    );
    assert!(config.get_dbname().unwrap().starts_with("dogrs_restore_"));
    let (client, connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
    let task = tokio::spawn(connection);
    let rows = client.query("SELECT tenant,state FROM dogrs_queue_state_v1 WHERE tenant LIKE 'dogrs-test-partition-%'", &[]).await.unwrap();
    let (tenant, record, token) = rows
        .iter()
        .find_map(|row| {
            let state: serde_json::Value = row.get(1);
            state["jobs"].as_object()?.values().find_map(|stored| {
                let record: dog_queue::JobRecord =
                    serde_json::from_value(stored["record"].clone()).ok()?;
                if !record.status.is_processing() {
                    return None;
                }
                let token: dog_queue::LeaseToken =
                    serde_json::from_value(stored["token"].clone()).ok()?;
                Some((QueueCtx::new(row.get::<_, String>(0)), record, token))
            })
        })
        .expect("snapshot must contain the interrupted partition job");
    let backend = PostgresBackend::new(PostgresConfig {
        connection_string: uri,
    })
    .await
    .unwrap();
    assert_eq!(
        backend
            .get_record(tenant.clone(), record.job_id.clone())
            .await
            .unwrap()
            .attempt,
        1
    );
    assert!(backend
        .ack_complete(tenant.clone(), record.job_id.clone(), token, None)
        .await
        .is_err());
    backend.reclaim_expired_leases().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let recovered = backend
        .dequeue(tenant.clone(), &["q"])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.record.job_id, record.job_id);
    backend
        .ack_complete(
            tenant.clone(),
            record.job_id.clone(),
            recovered.lease_token,
            None,
        )
        .await
        .unwrap();
    assert!(matches!(
        backend.get_status(tenant, record.job_id).await.unwrap(),
        JobStatus::Completed { .. }
    ));
    task.abort();
    println!("RESTORED_QUEUE_RECOVERY_VERIFIED");
}
