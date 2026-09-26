//! A synthetic billing system for hosted infrastructure validation. No real payments or email.
mod connections;
use anyhow::{bail, Context, Result};
use dog_core::{DogAppBuilder, DogService, TenantContext};
use dog_queue::{Job, JobError, JobId, QueueAdapter, QueueBackend, QueueConfig, QueueCtx};
use dog_transport::{HttpOptions, IntoDogService};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{borrow::Cow, sync::Arc, time::Duration};
use tokio_postgres::Client;

fn env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("missing {name}"))
}
fn tenant() -> Result<String> {
    let value = env("DOGRS_TEST_TENANT")?;
    anyhow::ensure!(
        value.starts_with("dogrs-test-") && value.len() < 100,
        "use a dedicated dogrs-test- tenant"
    );
    Ok(value)
}
const LEASE: Duration = Duration::from_secs(8);

#[derive(Clone)]
struct BillingContext {
    db: Arc<Client>,
    tenant: String,
    crash_after_effect: bool,
}
#[derive(Serialize, Deserialize)]
struct RecordPayment {
    invoice: String,
    mode: String,
    #[serde(default)]
    padding: String,
}
#[async_trait::async_trait]
impl Job for RecordPayment {
    type Context = BillingContext;
    type Result = Value;
    const JOB_TYPE: &'static str = "record-payment";
    fn idempotency_key(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(&self.invoice))
    }
    async fn execute(&self, ctx: BillingContext) -> Result<Value, JobError> {
        let fail = |e: tokio_postgres::Error| JobError::retryable(e.to_string());
        let row = ctx.db.query_one(
            "INSERT INTO dogrs_validation_attempts (tenant,invoice,attempts) VALUES ($1,$2,1) ON CONFLICT (tenant,invoice) DO UPDATE SET attempts=dogrs_validation_attempts.attempts+1 RETURNING attempts",
            &[&ctx.tenant, &self.invoice]).await.map_err(fail)?;
        let attempts: i32 = row.get(0);
        if self.mode == "retry" && attempts == 1 {
            return Err(JobError::retryable("synthetic first-attempt failure"));
        }
        if self.mode == "permanent" {
            return Err(JobError::permanent("synthetic rejected payment"));
        }
        if self.mode == "long" {
            tokio::time::sleep(Duration::from_secs(18)).await;
        }
        let inserted = ctx.db.execute(
            "INSERT INTO dogrs_validation_effects (tenant,invoice,worker) VALUES ($1,$2,$3) ON CONFLICT (tenant,invoice) DO NOTHING",
            &[&ctx.tenant, &self.invoice, &std::process::id().to_string()]).await.map_err(fail)?;
        if ctx.crash_after_effect && self.mode == "crash" {
            // The controller kills this entire process after it observes the committed effect.
            // Keep execution pending so DogRS has not acknowledged the job yet.
            std::future::pending::<()>().await;
        }
        Ok(json!({"invoice":self.invoice,"effect_inserted":inserted == 1}))
    }
}
struct BillingService<B: QueueBackend> {
    adapter: Arc<QueueAdapter<B>>,
    tenant: String,
}
#[async_trait::async_trait]
impl<B: QueueBackend + 'static> DogService<Value, ()> for BillingService<B> {
    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> Result<Value> {
        let job: RecordPayment = serde_json::from_value(data)?;
        anyhow::ensure!(
            !job.invoice.is_empty() && job.invoice.len() <= 100,
            "invalid synthetic invoice"
        );
        anyhow::ensure!(
            ["normal", "retry", "long", "crash", "permanent"].contains(&job.mode.as_str()),
            "invalid mode"
        );
        let id = self
            .adapter
            .enqueue(QueueCtx::new(&self.tenant), job)
            .await?;
        Ok(json!({"id":id}))
    }
    async fn get(&self, _: &TenantContext, id: &str, _: ()) -> Result<Value> {
        let record = self
            .adapter
            .backend()
            .get_record(QueueCtx::new(&self.tenant), JobId::from(id))
            .await?;
        Ok(
            json!({"id":record.job_id,"status":record.status.name(),"attempts":record.attempt,"result":record.result}),
        )
    }
    async fn remove(&self, _: &TenantContext, id: Option<&str>, _: ()) -> Result<Value> {
        let id = id.context("a single job ID is required")?;
        Ok(
            json!({"canceled":self.adapter.cancel(QueueCtx::new(&self.tenant),JobId::from(id)).await?}),
        )
    }
}
async fn run<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
    let tenant = tenant()?;
    let max_payload: usize = std::env::var("DOGRS_TEST_MAX_PAYLOAD")
        .unwrap_or_else(|_| "4096".into())
        .parse()?;
    anyhow::ensure!(
        (4096..=65536).contains(&max_payload),
        "payload limit must be 4–64 KiB"
    );
    let adapter = Arc::new(QueueAdapter::try_with_config(
        backend,
        QueueConfig {
            max_workers: 2,
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
#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_writer(std::io::stderr)
        .init();
    let role = std::env::args()
        .nth(1)
        .context("usage: hosted-system init|inspect|serve|worker")?;
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
