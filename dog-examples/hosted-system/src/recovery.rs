//! Controlled provider-process recovery against disposable local services only.
use super::*;
use dog_queue::{JobMessage, JobStatus, LeaseToken, QueueError};
use std::path::PathBuf;
#[derive(Serialize, Deserialize)]
struct Manifest {
    tenant: String,
    jobs: Vec<(JobId, u64)>,
    old_leases: Vec<(JobId, LeaseToken)>,
}
fn bytes(seed: u64) -> Vec<u8> {
    let mut bytes = vec![0; 65536];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (i as u64).wrapping_mul(131).wrapping_add(seed) as u8;
    }
    bytes
}
pub async fn run<B: QueueBackend>(backend: B, role: &str) -> Result<()> {
    let path = PathBuf::from(env("DOGRS_RECOVERY_MANIFEST")?);
    if role == "recovery-seed" || role == "recovery-live" {
        anyhow::ensure!(!path.exists(), "manifest already exists");
        let tenant = tenant()?;
        let ctx = QueueCtx::new(&tenant);
        let mut manifest = Manifest {
            tenant,
            jobs: Vec::new(),
            old_leases: Vec::new(),
        };
        for seed in 0..200u64 {
            let id = backend
                .enqueue(
                    ctx.clone(),
                    JobMessage::new("recovery", bytes(seed), "bytes", "recovery")
                        .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1))
                        .with_idempotency_key(seed.to_string()),
                )
                .await?;
            manifest.jobs.push((id, seed));
        }
        for n in 0..100 {
            let job = backend
                .dequeue(ctx.clone(), &["recovery"])
                .await?
                .context("missing seeded job")?;
            if n < 50 {
                backend
                    .ack_complete(
                        ctx.clone(),
                        job.record.job_id,
                        job.lease_token,
                        Some("committed-before-crash".into()),
                    )
                    .await?;
            } else {
                manifest
                    .old_leases
                    .push((job.record.job_id, job.lease_token));
            }
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
        println!("RECOVERY_READY acknowledged=200 completed=50 inflight=50 queued=100");
        if role == "recovery-seed" {
            return Ok(());
        }
        let resume = path.with_extension("resume");
        tokio::time::timeout(Duration::from_secs(180), async {
            while !resume.exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&path)?)?;
    let ctx = QueueCtx::new(&manifest.tenant);
    // The original backend remains alive in recovery-live, exercising reconnect.
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if backend
                .get_record(ctx.clone(), manifest.jobs[0].0.clone())
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await?;
    for (id, seed) in &manifest.jobs {
        let record = backend.get_record(ctx.clone(), id.clone()).await?;
        anyhow::ensure!(
            record.message.payload_bytes == bytes(*seed),
            "acknowledged payload missing or corrupt"
        );
    }
    for (id, token) in &manifest.old_leases {
        anyhow::ensure!(
            matches!(
                backend
                    .ack_complete(ctx.clone(), id.clone(), token.clone(), None)
                    .await,
                Err(QueueError::LeaseExpired)
                    | Err(QueueError::InvalidLeaseToken { .. })
                    | Err(QueueError::JobAlreadyTerminal)
            ),
            "expired owner accepted after outage"
        );
    }
    let reaped = backend.reclaim_expired_leases().await?;
    anyhow::ensure!(
        reaped
            .iter()
            .filter(|r| r.tenant_id == manifest.tenant)
            .count()
            == 50,
        "inflight recovery mismatch"
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let mut recovered = 0;
    while let Some(job) = backend.dequeue(ctx.clone(), &["recovery"]).await? {
        backend
            .ack_complete(
                ctx.clone(),
                job.record.job_id,
                job.lease_token,
                Some("recovered".into()),
            )
            .await?;
        recovered += 1;
    }
    anyhow::ensure!(
        recovered == 150,
        "queued/reclaimed job count mismatch: {recovered}"
    );
    let mut completed = 0;
    for (id, _) in &manifest.jobs {
        if matches!(
            backend.get_status(ctx.clone(), id.clone()).await?,
            JobStatus::Completed { .. }
        ) {
            completed += 1
        }
    }
    anyhow::ensure!(completed == 200, "terminal count mismatch");
    println!(
        "{}",
        json!({"acknowledged_before_crash":200,"payload_bytes":65536,"preserved_completed":50,"expired_owners_rejected":50,"recovered_jobs":150,"completed_after_recovery":completed,"same_backend_instance":role=="recovery-live","passed":true})
    );
    Ok(())
}
