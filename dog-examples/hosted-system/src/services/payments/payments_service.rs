use crate::app::*;
pub(crate) struct BillingService<B: QueueBackend> {
    pub(crate) adapter: Arc<QueueAdapter<B>>,
    pub(crate) tenant: String,
}
#[async_trait::async_trait]
impl<B: QueueBackend + 'static> DogService<Value, ()> for BillingService<B> {
    async fn create(&self, _: &TenantContext, data: Value, _: ()) -> Result<Value> {
        if let Some(ids) = data.get("status_ids") {
            let ids: Vec<String> = serde_json::from_value(ids.clone())?;
            anyhow::ensure!(
                ids.len() <= 1000 && ids.iter().all(|id| !id.is_empty()),
                "invalid status batch"
            );
            let ids: Vec<JobId> = ids.into_iter().map(JobId::from).collect();
            let snapshots = self
                .adapter
                .backend()
                .get_snapshots(QueueCtx::new(&self.tenant), &ids)
                .await?;
            let rows: Vec<Value>=snapshots.into_iter().map(|r|json!({"id":r.job_id,"status":r.status.name(),"attempts":r.attempt,"result":r.result})).collect();
            return Ok(json!({"snapshots":rows}));
        }
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
            .get_snapshot(QueueCtx::new(&self.tenant), JobId::from(id))
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
