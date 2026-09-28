//! Billing service implementation and business operations.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use dog_core::{DogService, ServiceCapabilities, TenantContext};
use dog_queue::{Job, JobError, JobId, QueueAdapter, QueueBackend, QueueCtx};
use serde_json::{json, Value};
use tokio_postgres::Client;

use super::payments_schema::{validate_status_batch, RecordPayment};
use crate::services::types::BillingContext;

pub struct BillingService<B: QueueBackend> {
    pub adapter: Arc<QueueAdapter<B>>,
    pub tenant: String,
}

impl<B: QueueBackend> BillingService<B> {
    pub fn new(adapter: Arc<QueueAdapter<B>>, tenant: String) -> Self {
        Self { adapter, tenant }
    }
}

#[async_trait::async_trait]
impl<B: QueueBackend + 'static> DogService<Value, ()> for BillingService<B> {
    fn capabilities(&self) -> ServiceCapabilities {
        super::payments_shared::capabilities()
    }

    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> Result<Value> {
        if let Some(ids) = data.get("status_ids") {
            let ids: Vec<String> = serde_json::from_value(ids.clone())?;
            validate_status_batch(&ids)?;
            let ids: Vec<JobId> = ids.into_iter().map(JobId::from).collect();
            let snapshots = self
                .adapter
                .backend()
                .get_snapshots(QueueCtx::new(&self.tenant), &ids)
                .await?;
            let rows: Vec<Value> = snapshots
                .into_iter()
                .map(|r| {
                    json!({
                        "id": r.job_id,
                        "status": r.status.name(),
                        "attempts": r.attempt,
                        "result": r.result
                    })
                })
                .collect();
            return Ok(json!({ "snapshots": rows }));
        }

        let job: RecordPayment = serde_json::from_value(data)?;
        job.validate()?;

        let id = self
            .adapter
            .enqueue(QueueCtx::new(&self.tenant), job)
            .await?;
        Ok(json!({ "id": id }))
    }

    async fn get(&self, _: &TenantContext, id: &str, _: ()) -> Result<Value> {
        let record = self
            .adapter
            .backend()
            .get_snapshot(QueueCtx::new(&self.tenant), JobId::from(id))
            .await?;
        Ok(json!({
            "id": record.job_id,
            "status": record.status.name(),
            "attempts": record.attempt,
            "result": record.result
        }))
    }

    async fn remove(&self, _: &TenantContext, id: Option<&str>, _: ()) -> Result<Value> {
        let id = id.context("a single job ID is required")?;
        Ok(json!({
            "canceled": self.adapter.cancel(QueueCtx::new(&self.tenant), JobId::from(id)).await?
        }))
    }
}

async fn record_attempt(
    client: &Client,
    tenant: &str,
    invoice: &str,
) -> Result<i32, tokio_postgres::Error> {
    let row = client
        .query_one(
            "INSERT INTO dogrs_validation_attempts (tenant,invoice,attempts) VALUES ($1,$2,1) ON CONFLICT (tenant,invoice) DO UPDATE SET attempts=dogrs_validation_attempts.attempts+1 RETURNING attempts",
            &[&tenant, &invoice],
        )
        .await?;
    Ok(row.get(0))
}

async fn record_effect(
    client: &Client,
    tenant: &str,
    invoice: &str,
    worker_id: &str,
) -> Result<u64, tokio_postgres::Error> {
    client
        .execute(
            "INSERT INTO dogrs_validation_effects (tenant,invoice,worker) VALUES ($1,$2,$3) ON CONFLICT (tenant,invoice) DO NOTHING",
            &[&tenant, &invoice, &worker_id],
        )
        .await
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
        let attempts = record_attempt(&ctx.db, &ctx.tenant, &self.invoice)
            .await
            .map_err(fail)?;

        if self.mode == "retry" && attempts == 1 {
            return Err(JobError::retryable("synthetic first-attempt failure"));
        }
        if self.mode == "permanent" {
            return Err(JobError::permanent("synthetic rejected payment"));
        }
        if self.mode == "long" {
            tokio::time::sleep(Duration::from_secs(18)).await;
        }

        let inserted = record_effect(
            &ctx.db,
            &ctx.tenant,
            &self.invoice,
            &std::process::id().to_string(),
        )
        .await
        .map_err(fail)?;

        if ctx.crash_after_effect && self.mode == "crash" {
            // The controller kills this entire process after it observes the committed effect.
            // Keep execution pending so DogRS has not acknowledged the job yet.
            std::future::pending::<()>().await;
        }

        Ok(json!({
            "invoice": self.invoice,
            "effect_inserted": inserted == 1
        }))
    }
}
