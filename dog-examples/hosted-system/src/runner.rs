//! Acceptance test harness runner, process orchestration, and role execution.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use dog_queue::{Job, JobId, QueueAdapter, QueueBackend, QueueConfig, QueueCtx};

use crate::services::adapters::PaymentsAdapter;
use crate::services::types::BillingContext;
use crate::services::RecordPayment;
use crate::{capacity, connections};

pub const LEASE: Duration = Duration::from_secs(8);

pub fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("missing {name}"))
}

pub fn tenant() -> Result<String> {
    let value = env("DOGRS_TEST_TENANT")?;
    anyhow::ensure!(
        value.starts_with("dogrs-test-") && value.len() < 100,
        "use a dedicated dogrs-test- tenant"
    );
    Ok(value)
}

/// Builds and registers a configured QueueAdapter for harness execution.
pub async fn build_queue_adapter<B: QueueBackend + 'static>(
    backend: B,
) -> Result<Arc<QueueAdapter<B>>> {
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
    Ok(adapter)
}

/// Runs the HTTP API service with Bearer token authentication and loopback binding.
pub async fn serve<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<()> {
    let token = env("DOGRS_TEST_TOKEN")?;
    anyhow::ensure!(
        token.len() >= 32,
        "test API bearer token must be at least 32 characters"
    );
    let (_app, service) = crate::build_with(adapter, tenant).await?;
    let expected_auth = format!("Bearer {token}");

    let router = axum::Router::new()
        .fallback_service(service)
        .layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let expected = expected_auth.clone();
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
    Ok(())
}

/// Runs background workers for billing payment processing with graceful shutdown.
pub async fn run_worker<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
) -> Result<()> {
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
    Ok(())
}

/// Verifies tenant boundary isolation for the test job.
pub async fn verify_isolation<B: QueueBackend + 'static>(
    adapter: Arc<QueueAdapter<B>>,
    tenant: &str,
) -> Result<()> {
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
    Ok(())
}

/// Executes the requested role against the given queue backend.
pub async fn run<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
    if role == "capacity" {
        return capacity::run(backend).await;
    }
    let tenant = tenant()?;
    let adapter = build_queue_adapter(backend).await?;

    match role {
        "serve" => serve(adapter, tenant).await,
        "worker" => run_worker(adapter, tenant).await,
        "isolation" => verify_isolation(adapter, &tenant).await,
        _ => bail!("role must be serve, worker or isolation"),
    }
}

/// Initializes the synthetic billing schema in PostgreSQL.
pub async fn init_schema() -> Result<()> {
    let db = connections::postgres_client().await?;
    PaymentsAdapter::init_schema(&db).await?;
    println!("SYNTHETIC_SCHEMA_READY");
    Ok(())
}

/// Inspects recorded payment rows for the configured tenant.
pub async fn inspect_schema() -> Result<()> {
    let db = connections::postgres_client().await?;
    let rows = PaymentsAdapter::inspect_schema(&db, &tenant()?).await?;
    println!("{}", serde_json::to_string(&rows)?);
    Ok(())
}
