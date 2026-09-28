//! A synthetic billing system for hosted infrastructure validation. No real payments or email.
pub(crate) use crate::services::{payments::payments_service::BillingService, types::*};
pub(crate) use crate::{admission, capacity, connections, recovery};
pub(crate) use anyhow::{bail, Context, Result};
pub(crate) use dog_core::{DogAppBuilder, DogService, TenantContext};
pub(crate) use dog_queue::{
    Job, JobError, JobId, QueueAdapter, QueueBackend, QueueConfig, QueueCtx,
};
pub(crate) use dog_transport::{HttpOptions, IntoDogService};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{json, Value};
pub(crate) use std::{borrow::Cow, sync::Arc, time::Duration};
pub(crate) use tokio_postgres::Client;

pub(crate) fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("missing {name}"))
}
pub(crate) fn tenant() -> Result<String> {
    let value = env("DOGRS_TEST_TENANT")?;
    anyhow::ensure!(
        value.starts_with("dogrs-test-") && value.len() < 100,
        "use a dedicated dogrs-test- tenant"
    );
    Ok(value)
}
pub(crate) const LEASE: Duration = Duration::from_secs(8);

pub(crate) async fn run<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
    if role == "capacity" {
        return capacity::run(backend).await;
    }
    let tenant = tenant()?;
    let max_payload: usize = std::env::var("DOGRS_TEST_MAX_PAYLOAD")
        .unwrap_or_else(|_| "4096".into())
        .parse()?;
    anyhow::ensure!(
        (4096..=65536).contains(&max_payload),
        "payload limit must be 4–64 KiB"
    );
    let workers: usize = std::env::var("DOGRS_TEST_WORKERS")
        .unwrap_or_else(|_| "2".into())
        .parse()?;
    anyhow::ensure!((1..=32).contains(&workers), "worker count must be 1–32");
    let adapter = Arc::new(QueueAdapter::try_with_config(
        backend,
        QueueConfig {
            max_workers: workers,
            lease_duration: LEASE,
            heartbeat_interval: Duration::from_secs(2),
            poll_interval: Duration::from_millis(250),
            poll_jitter: Duration::from_millis(25),
            worker_idle_timeout: Duration::from_secs(300),
            max_payload_size: Some(max_payload),
            ..Default::default()
        },
    )?);
    adapter.register_job::<RecordPayment>().await?;
    match role {
        "serve" => {
            let token = env("DOGRS_TEST_TOKEN")?;
            anyhow::ensure!(
                token.len() >= 32,
                "test API bearer token must be at least 32 characters"
            );
            let mut builder = DogAppBuilder::<Value, ()>::new();
            builder.register_service("payments", Arc::new(BillingService { adapter, tenant }));
            let service = builder.build().into_service(HttpOptions::new());
            let router =
                axum::Router::new()
                    .fallback_service(service)
                    .layer(axum::middleware::from_fn(
                        move |request: axum::extract::Request, next: axum::middleware::Next| {
                            let expected = format!("Bearer {token}");
                            async move {
                                if request
                                    .headers()
                                    .get(axum::http::header::AUTHORIZATION)
                                    .and_then(|v| v.to_str().ok())
                                    != Some(expected.as_str())
                                {
                                    return axum::response::IntoResponse::into_response(
                                        axum::http::StatusCode::UNAUTHORIZED,
                                    );
                                }
                                next.run(request).await
                            }
                        },
                    ));
            // Loopback only. A deployed reverse proxy must provide authenticated HTTPS.
            let address = env("DOGRS_TEST_BIND").unwrap_or_else(|_| "127.0.0.1:38171".into());
            let listener = tokio::net::TcpListener::bind(&address).await?;
            println!("API_READY {address}");
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        "worker" => {
            let db = Arc::new(connections::postgres_client().await?);
            let ctx = BillingContext {
                db,
                tenant: tenant.clone(),
                crash_after_effect: std::env::var("DOGRS_CRASH_AFTER_EFFECT").as_deref() == Ok("1"),
            };
            let handle = adapter
                .start_workers(
                    QueueCtx::new(tenant),
                    ctx,
                    vec![RecordPayment::JOB_TYPE.into()],
                )
                .await?;
            println!("WORKER_READY");
            tokio::signal::ctrl_c().await?;
            handle.shutdown().await?;
        }
        "isolation" => {
            let result = adapter
                .backend()
                .get_status(
                    QueueCtx::new(format!("{tenant}-other")),
                    JobId::from(env("DOGRS_TEST_JOB")?),
                )
                .await;
            anyhow::ensure!(
                matches!(result, Err(dog_queue::QueueError::JobNotFound(_))),
                "tenant isolation failed"
            );
            println!("TENANT_ISOLATION_VERIFIED");
        }
        _ => bail!("role must be serve, worker or isolation"),
    }
    Ok(())
}
pub async fn run_app() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .init();
    let role = std::env::args()
        .nth(1)
        .context("usage: hosted-system init|inspect|serve|worker")?;
    if role == "admission-native" {
        return admission::native().await;
    }
    if role == "capacity-local" || role.starts_with("recovery-") {
        let backend = env("DOGRS_BACKEND")?;
        match backend.as_str() {
            "postgres" => {
                let uri = env("DOGRS_POSTGRES_URL")?;
                anyhow::ensure!(
                    uri.contains("127.0.0.1") || uri.contains("localhost"),
                    "local capacity requires loopback"
                );
                let mut options = dog_queue::backend::postgres::PostgresOptions {
                    payload_storage: match std::env::var("DOGRS_PG_PAYLOAD_STORAGE").as_deref() {
                        Ok("external") => {
                            Some(dog_queue::backend::postgres::PostgresPayloadStorage::External)
                        }
                        Ok("extended") => {
                            Some(dog_queue::backend::postgres::PostgresPayloadStorage::Extended)
                        }
                        Err(std::env::VarError::NotPresent) => None,
                        _ => bail!("PostgreSQL payload storage must be external or extended"),
                    },
                    max_connections: std::env::var("DOGRS_PG_POOL_SIZE")
                        .unwrap_or_else(|_| "64".into())
                        .parse()?,
                    enqueue_concurrency: std::env::var("DOGRS_PG_ENQUEUE_CONCURRENCY")
                        .ok()
                        .map(|n| n.parse())
                        .transpose()?,
                    operation_timeout: Duration::from_secs(10),
                    ..Default::default()
                };
                let shards: u32 = std::env::var("DOGRS_CAPACITY_SHARDS")
                    .unwrap_or_else(|_| "1".into())
                    .parse()?;
                anyhow::ensure!(
                    (1..=16).contains(&shards),
                    "PostgreSQL shards must be 1..=16"
                );
                if shards > 1 {
                    anyhow::ensure!(
                        role == "capacity-local",
                        "sharded topology is for the capacity fixture"
                    );
                    anyhow::ensure!(
                        options.max_connections >= shards
                            && options.max_connections.is_multiple_of(shards),
                        "total pool size must divide evenly across shards"
                    );
                    options.max_connections /= shards;
                    options.batch_concurrency = Some((4 / shards as usize).max(1));
                    if let Some(cap) = options.enqueue_concurrency {
                        anyhow::ensure!(
                            cap >= shards && cap.is_multiple_of(shards),
                            "producer cap must divide evenly across shards"
                        );
                        options.enqueue_concurrency = Some(cap / shards);
                    }
                    let shard_urls: Vec<String> = std::env::var("DOGRS_PG_SHARD_URLS")
                        .ok()
                        .map(|value| serde_json::from_str(&value))
                        .transpose()?
                        .unwrap_or_else(|| vec![uri.clone(); shards as usize]);
                    anyhow::ensure!(
                        shard_urls.len() == shards as usize,
                        "one PostgreSQL URL per fixed shard is required"
                    );
                    for url in &shard_urls {
                        let config: tokio_postgres::Config = url.parse()?;
                        anyhow::ensure!(!config.get_hosts().is_empty() && config.get_hosts().iter().all(|host| matches!(host, tokio_postgres::config::Host::Tcp(host) if host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()))), "capacity shard URLs must use loopback hosts");
                    }
                    let mut backends = Vec::new();
                    for (shard, connection_string) in shard_urls.into_iter().enumerate() {
                        options.schema = Some(format!("dogrs_capacity_{shard}"));
                        backends.push(Arc::new(
                            dog_queue::backend::postgres::PostgresBackend::new_with_tls_options(
                                dog_queue::backend::postgres::PostgresConfig { connection_string },
                                tokio_postgres::NoTls,
                                options.clone(),
                            )
                            .await?,
                        ));
                    }
                    return run_local(
                        dog_queue::backend::sharded::ShardedBackend::new(backends)?,
                        &role,
                    )
                    .await;
                }
                let backend = dog_queue::backend::postgres::PostgresBackend::new_with_tls_options(
                    dog_queue::backend::postgres::PostgresConfig {
                        connection_string: uri,
                    },
                    tokio_postgres::NoTls,
                    options,
                )
                .await?;
                return run_local(
                    backend.with_lease_duration(Duration::from_secs(if role == "capacity-local" {
                        300
                    } else {
                        2
                    })),
                    &role,
                )
                .await;
            }
            #[cfg(feature = "redis")]
            "redis" => {
                let uri = env("DOGRS_REDIS_URL")?;
                anyhow::ensure!(
                    uri.contains("127.0.0.1") || uri.contains("localhost"),
                    "local capacity requires loopback"
                );
                let backend = dog_queue::backend::redis::RedisBackend::new(
                    dog_queue::backend::redis::RedisConfig {
                        connection_string: uri,
                    },
                )
                .await?
                .with_lease_duration(Duration::from_secs(
                    if role == "capacity-local" { 300 } else { 2 },
                ));
                if std::env::var("DOGRS_REDIS_REQUIRE_AOF").as_deref() == Ok("1") {
                    backend.verify_persistence().await?;
                }
                return run_local(backend, &role).await;
            }
            #[cfg(feature = "nats")]
            "nats" => {
                let uri = env("DOGRS_NATS_URL")?;
                anyhow::ensure!(
                    uri.contains("127.0.0.1") || uri.contains("localhost"),
                    "local recovery requires loopback"
                );
                let name = env("DOGRS_NATS_BUCKET")?;
                let client = async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?;
                let js = async_nats::jetstream::new(client);
                let shards: usize = std::env::var("DOGRS_CAPACITY_SHARDS")
                    .unwrap_or_else(|_| "1".into())
                    .parse()?;
                anyhow::ensure!((1..=32).contains(&shards), "shards must be 1–32");
                let connections =
                    std::env::var("DOGRS_NATS_CONNECTIONS").unwrap_or_else(|_| "shared".into());
                anyhow::ensure!(
                    matches!(connections.as_str(), "shared" | "per-shard"),
                    "NATS connections must be shared or per-shard"
                );
                if shards > 1 {
                    anyhow::ensure!(
                        role == "capacity-local",
                        "sharded topology is for the capacity fixture"
                    );
                    let mut backends = Vec::new();
                    for shard in 0..shards {
                        let js = if connections == "per-shard" && shard > 0 {
                            async_nats::jetstream::new(
                                async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?,
                            )
                        } else {
                            js.clone()
                        };
                        let name = format!("{name}_{shard}");
                        let bucket = create_fixture_bucket(
                            &js,
                            async_nats::jetstream::kv::Config {
                                bucket: name.clone(),
                                num_replicas: std::env::var("DOGRS_NATS_REPLICAS")
                                    .unwrap_or_else(|_| "3".into())
                                    .parse()?,
                                storage: async_nats::jetstream::stream::StorageType::File,
                                history: 1,
                                ..Default::default()
                            },
                        )
                        .await?;
                        let mut config = bucket.stream.cached_info().config.clone();
                        config.allow_direct = false;
                        config.allow_atomic_publish =
                            std::env::var("DOGRS_NATS_ATOMIC").as_deref() != Ok("0");
                        js.update_stream(config).await?;
                        backends.push(Arc::new(
                            dog_queue::backend::nats::NatsBackend::from_context(
                                js.clone(),
                                &name,
                                1024 * 1024,
                            )
                            .await?,
                        ));
                    }
                    return capacity::run(dog_queue::backend::sharded::ShardedBackend::new(
                        backends,
                    )?)
                    .await;
                }
                let bucket = match js.get_key_value(&name).await {
                    Ok(bucket) => bucket,
                    Err(_) => {
                        create_fixture_bucket(
                            &js,
                            async_nats::jetstream::kv::Config {
                                bucket: name.clone(),
                                num_replicas: std::env::var("DOGRS_NATS_REPLICAS")
                                    .unwrap_or_else(|_| "1".into())
                                    .parse()?,
                                storage: async_nats::jetstream::stream::StorageType::File,
                                history: 1,
                                ..Default::default()
                            },
                        )
                        .await?
                    }
                };
                let mut config = bucket.stream.cached_info().config.clone();
                config.allow_direct = false;
                config.allow_atomic_publish =
                    std::env::var("DOGRS_NATS_ATOMIC").as_deref() != Ok("0");
                js.update_stream(config).await?;
                let backend = dog_queue::backend::nats::NatsBackend::from_context(
                    js.clone(),
                    &name,
                    1024 * 1024,
                )
                .await?
                .with_lease_duration(Duration::from_secs(
                    if role == "capacity-local" { 300 } else { 2 },
                ));
                return run_local(backend, &role).await;
            }
            _ => bail!("unsupported local capacity backend"),
        }
    }
    if role == "network-probe" {
        // Isolate PostgreSQL transport latency from queue transactions and workers.
        let db = Arc::new(connections::postgres_client().await?);
        for bytes in [1024usize, 16384, 65536] {
            let payload = Arc::new(vec![42u8; bytes]);
            let mut tasks = tokio::task::JoinSet::new();
            let start = std::time::Instant::now();
            for index in 0..100u32 {
                tokio::time::sleep_until(tokio::time::Instant::from_std(
                    start + Duration::from_millis(u64::from(index) * 100),
                ))
                .await;
                let db = db.clone();
                let payload = payload.clone();
                tasks.spawn(async move {
                    let start = std::time::Instant::now();
                    let row = db
                        .query_typed_one(
                            "SELECT octet_length($1::bytea)",
                            &[(&*payload, tokio_postgres::types::Type::BYTEA)],
                        )
                        .await?;
                    anyhow::ensure!(
                        row.get::<_, i32>(0) == payload.len() as i32,
                        "unexpected transport echo"
                    );
                    Ok::<_, anyhow::Error>(start.elapsed().as_secs_f64() * 1000.0)
                });
            }
            let mut timings = vec![];
            while let Some(result) = tasks.join_next().await {
                timings.push(result??);
            }
            timings.sort_by(f64::total_cmp);
            println!(
                "{}",
                json!({"probe":"postgres_transport_only","payload_bytes":bytes,"requests":100,"offered_rps":10,"seconds":start.elapsed().as_secs_f64(),"p50_ms":timings[49],"p95_ms":timings[94]})
            );
        }
        return Ok(());
    }
    if role == "init" || role == "inspect" {
        let db = connections::postgres_client().await?;
        if role == "init" {
            db.batch_execute("CREATE TABLE IF NOT EXISTS dogrs_validation_attempts (tenant TEXT NOT NULL, invoice TEXT NOT NULL, attempts INTEGER NOT NULL, PRIMARY KEY (tenant,invoice)); CREATE TABLE IF NOT EXISTS dogrs_validation_effects (tenant TEXT NOT NULL, invoice TEXT NOT NULL, worker TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (tenant,invoice));").await?;
            println!("SYNTHETIC_SCHEMA_READY");
        } else {
            let rows = db.query("SELECT a.invoice,a.attempts,e.worker FROM dogrs_validation_attempts a LEFT JOIN dogrs_validation_effects e USING (tenant,invoice) WHERE a.tenant=$1 ORDER BY a.invoice", &[&tenant()?]).await?;
            let result: Vec<Value> = rows.iter().map(|r|json!({"invoice":r.get::<_,String>(0),"attempts":r.get::<_,i32>(1),"worker":r.get::<_,Option<String>>(2)})).collect();
            println!("{}", serde_json::to_string(&result)?);
        }
        return Ok(());
    }
    connections::dispatch(&role).await
}

// Cluster discovery can elect a leader before peer placement becomes available.
// Retry only that structured transient error; quota/auth/config failures stay fatal.
#[cfg(feature = "nats")]
pub(crate) async fn create_fixture_bucket(
    js: &async_nats::jetstream::Context,
    config: async_nats::jetstream::kv::Config,
) -> Result<async_nats::jetstream::kv::Store> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match js.create_key_value(config.clone()).await {
            Ok(bucket) => return Ok(bucket),
            Err(error) => {
                let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                let mut placement_pending = false;
                while let Some(current) = cause {
                    if let Some(server) = current.downcast_ref::<async_nats::jetstream::Error>() {
                        placement_pending = server.error_code()
                            == async_nats::jetstream::ErrorCode::CLUSTER_NO_PEERS;
                    }
                    cause = current.source();
                }
                if !placement_pending || tokio::time::Instant::now() >= deadline {
                    return Err(error.into());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

async fn run_local<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
    if role.starts_with("recovery-") {
        recovery::run(backend, role).await
    } else {
        capacity::run(backend).await
    }
}
