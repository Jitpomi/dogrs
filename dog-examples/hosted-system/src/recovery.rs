//! Controlled provider-process recovery against disposable local services only.
use crate::runner::*;
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
pub async fn run<B: QueueBackend + 'static>(backend: B, role: &str) -> Result<()> {
    if role == "recovery-race" {
        let backend = Arc::new(backend);
        let prefix = tenant()?;
        let mut tasks = tokio::task::JoinSet::new();
        for t in 0..32 {
            let backend = backend.clone();
            let ctx = QueueCtx::new(format!("{prefix}-{t}"));
            tasks.spawn(async move {
                for n in 0..20 {
                    let id = backend
                        .enqueue(
                            ctx.clone(),
                            JobMessage::new("race", bytes(n), "bytes", "race")
                                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1))
                                .with_idempotency_key(n.to_string()),
                        )
                        .await?;
                    let (reads, completion) = tokio::join!(
                        async {
                            for _ in 0..10 {
                                backend.get_snapshot(ctx.clone(), id.clone()).await?;
                                tokio::task::yield_now().await;
                            }
                            Ok::<_, dog_queue::QueueError>(())
                        },
                        async {
                            let job = loop {
                                if let Some(job) = backend.dequeue(ctx.clone(), &["race"]).await? {
                                    break job;
                                }
                                tokio::time::sleep(Duration::from_millis(10)).await;
                            };
                            backend
                                .ack_complete(ctx.clone(), job.record.job_id, job.lease_token, None)
                                .await
                        }
                    );
                    reads?;
                    completion?;
                    backend.get_record(ctx.clone(), id).await?;
                }
                Ok::<_, dog_queue::QueueError>(())
            });
        }
        while let Some(result) = tasks.join_next().await {
            result??;
        }
        println!("RECORD_ARCHIVE_RACE_PASSED jobs=640 readers_per_job=10");
        return Ok(());
    }
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
        use futures::StreamExt;
        let mut submissions = futures::stream::iter(0..200u64)
            .map(|seed| {
                let ctx = ctx.clone();
                let backend = &backend;
                async move {
                    let id = backend
                        .enqueue(
                            ctx,
                            JobMessage::new("recovery", bytes(seed), "bytes", "recovery")
                                .with_run_at(chrono::Utc::now() - chrono::Duration::seconds(1))
                                .with_idempotency_key(seed.to_string()),
                        )
                        .await?;
                    Ok::<_, dog_queue::QueueError>((id, seed))
                }
            })
            .buffer_unordered(16);
        while let Some(result) = submissions.next().await {
            manifest.jobs.push(result?);
        }
        drop(submissions);
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
        let mut unavailable = 0usize;
        tokio::time::timeout(Duration::from_secs(600), async {
            while !resume.exists() {
                if !matches!(
                    tokio::time::timeout(
                        Duration::from_secs(3),
                        backend.get_record(ctx.clone(), manifest.jobs[0].0.clone())
                    )
                    .await,
                    Ok(Ok(_))
                ) {
                    unavailable += 1;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await?;
        println!("RECOVERY_OUTAGE_PROBES_FAILED {unavailable}");
    }
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(&path)?)?;
    let ctx = QueueCtx::new(&manifest.tenant);
    // The original backend remains alive in recovery-live, exercising reconnect.
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            match backend
                .get_record(ctx.clone(), manifest.jobs[0].0.clone())
                .await
            {
                Ok(_) => return,
                Err(error) => eprintln!("RECOVERY_RECONNECT_RETRY {error}"),
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
