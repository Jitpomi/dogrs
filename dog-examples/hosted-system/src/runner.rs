//! Acceptance test harness runner, process orchestration, and role execution.

pub use std::sync::Arc;
pub use std::time::Duration;

pub use anyhow::{bail, Context, Result};
pub use dog_queue::{Job, JobId, QueueAdapter, QueueBackend, QueueConfig, QueueCtx};
pub use dog_transport::{HttpOptions, IntoDogService};
pub use serde::{Deserialize, Serialize};
pub use serde_json::{json, Value};
pub use tokio_postgres::Client;

use crate::services::adapters::PaymentsAdapter;
use crate::services::types::BillingContext;
use crate::services::RecordPayment;
use crate::{admission, capacity, connections};

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

pub async fn run<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
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
            let (_app, service) = crate::build(adapter, tenant).await?;
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

pub async fn dispatch_role(role: &str) -> Result<()> {
    if role == "admission-native" {
        return admission::native().await;
    }
    if role == "capacity-local" || role.starts_with("recovery-") {
        return connections::dispatch_local(role).await;
    }
    if role == "network-probe" {
        return connections::network_probe().await;
    }
    if role == "init" {
        let db = connections::postgres_client().await?;
        PaymentsAdapter::init_schema(&db).await?;
        println!("SYNTHETIC_SCHEMA_READY");
        return Ok(());
    }
    if role == "inspect" {
        let db = connections::postgres_client().await?;
        let rows = PaymentsAdapter::inspect_schema(&db, &tenant()?).await?;
        println!("{}", serde_json::to_string(&rows)?);
        return Ok(());
    }
    connections::dispatch(role).await
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
    dispatch_role(&role).await
}
