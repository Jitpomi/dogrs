//! Queue-level open-loop test. This measures real backend persistence and payload
//! delivery; it is separate from the HTTP/PostgreSQL billing-effect acceptance test.
use super::*;
use dog_queue::JobMessage;
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Instant,
};

pub async fn run<B: QueueBackend + 'static>(backend: B) -> Result<()> {
    let tenants: usize = std::env::var("DOGRS_CAPACITY_TENANTS")
        .unwrap_or_else(|_| "100".into())
        .parse()?;
    let seconds: usize = std::env::var("DOGRS_CAPACITY_SECONDS")
        .unwrap_or_else(|_| "30".into())
        .parse()?;
    let bytes: usize = std::env::var("DOGRS_CAPACITY_BYTES")
        .unwrap_or_else(|_| "1024".into())
        .parse()?;
    let workers: usize = std::env::var("DOGRS_CAPACITY_WORKERS")
        .unwrap_or_else(|_| "4".into())
        .parse()?;
    let inflight: usize = std::env::var("DOGRS_CAPACITY_INFLIGHT")
        .unwrap_or_else(|_| "32".into())
        .parse()?;
    anyhow::ensure!(
        (1..=100).contains(&tenants)
            && (1..=120).contains(&seconds)
            && (16..=65536).contains(&bytes)
            && (1..=16).contains(&workers)
            && (1..=64).contains(&inflight),
        "invalid capacity bounds"
    );
    let prefix = tenant()?;
    let backend = Arc::new(backend);
    let accepted = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let overload = Arc::new(AtomicUsize::new(0));
    let late = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let latency = Arc::new(Mutex::new(Vec::new()));
    let ids = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now() + Duration::from_secs(1);
    let deadline = started + Duration::from_secs(seconds as u64 + 5);
    let mut consumers = tokio::task::JoinSet::new();
    for t in 0..tenants {
        for _ in 0..workers {
            let backend = backend.clone();
            let tenant = format!("{prefix}-{t}");
            let completed = completed.clone();
            let errors = errors.clone();
            let seen = seen.clone();
            consumers.spawn(async move {
                while Instant::now() < deadline {
                    match backend.dequeue(QueueCtx::new(&tenant), &["capacity"]).await {
                        Ok(Some(job)) => {
                            let result: Result<()> = async {
                                anyhow::ensure!(
                                    job.record.tenant_id == tenant,
                                    "tenant isolation failure"
                                );
                                let payload = &job.record.message.payload_bytes;
                                anyhow::ensure!(payload.len() == bytes, "payload length mismatch");
                                let seed = u64::from_le_bytes(payload[..8].try_into().unwrap());
                                anyhow::ensure!(
                                    *payload == make_payload(seed, bytes),
                                    "payload corruption"
                                );
                                anyhow::ensure!(
                                    seen.lock().unwrap().insert((tenant.clone(), seed)),
                                    "duplicate execution"
                                );
                                backend
                                    .ack_complete(
                                        QueueCtx::new(&tenant),
                                        job.record.job_id,
                                        job.lease_token,
                                        None,
                                    )
                                    .await?;
                                completed.fetch_add(1, Ordering::SeqCst);
                                Ok(())
                            }
                            .await;
                            if let Err(e) = result {
                                errors.lock().unwrap().push(format!("execute/ack: {e}"));
                            }
                        }
                        Ok(None) => tokio::time::sleep(Duration::from_millis(50)).await,
                        Err(e) => {
                            errors.lock().unwrap().push(format!("dequeue: {e}"));
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            });
        }
    }
    let mut producers = tokio::task::JoinSet::new();
    for t in 0..tenants {
        let backend = backend.clone();
        let tenant = format!("{prefix}-{t}");
        let accepted = accepted.clone();
        let overload = overload.clone();
        let late = late.clone();
        let errors = errors.clone();
        let latency = latency.clone();
        let ids = ids.clone();
        producers.spawn(async move {
            let slots = Arc::new(tokio::sync::Semaphore::new(inflight));
            let mut requests = tokio::task::JoinSet::new();
            for n in 0..seconds * 10 {
                let due = started
                    + Duration::from_millis(n as u64 * 100 + t as u64 * 100 / tenants as u64);
                tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
                if Instant::now().saturating_duration_since(due) > Duration::from_millis(100) {
                    late.fetch_add(1, Ordering::SeqCst);
                }
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    overload.fetch_add(1, Ordering::SeqCst);
                    continue;
                };
                let backend = backend.clone();
                let tenant = tenant.clone();
                let accepted = accepted.clone();
                let errors = errors.clone();
                let latency = latency.clone();
                let ids = ids.clone();
                requests.spawn(async move {
                    let _permit = permit;
                    let begin = Instant::now();
                    let message = JobMessage::new(
                        "capacity",
                        make_payload(n as u64, bytes),
                        "bytes",
                        "capacity",
                    )
                    .with_idempotency_key(n.to_string());
                    match backend.enqueue(QueueCtx::new(&tenant), message).await {
                        Ok(id) => {
                            accepted.fetch_add(1, Ordering::SeqCst);
                            ids.lock().unwrap().push((tenant, id));
                        }
                        Err(e) => errors.lock().unwrap().push(format!("enqueue: {e}")),
                    }
                    latency
                        .lock()
                        .unwrap()
                        .push(begin.elapsed().as_secs_f64() * 1000.0);
                });
            }
            while let Some(result) = requests.join_next().await {
                result.unwrap();
            }
        });
    }
    while let Some(result) = producers.join_next().await {
        result?;
    }
    let offered = tenants * seconds * 10;
    while completed.load(Ordering::SeqCst) < offered && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let elapsed = started.elapsed().as_secs_f64();
    consumers.abort_all();
    while consumers.join_next().await.is_some() {}
    let mut verified = 0;
    let ids = Arc::try_unwrap(ids).unwrap().into_inner().unwrap();
    for t in 0..tenants {
        let tenant = format!("{prefix}-{t}");
        let keys: Vec<_> = ids
            .iter()
            .filter(|(name, _)| name == &tenant)
            .map(|(_, id)| id.clone())
            .collect();
        for chunk in keys.chunks(1000) {
            match backend.get_snapshots(QueueCtx::new(&tenant), chunk).await {
                Ok(rows) => {
                    verified += rows
                        .iter()
                        .filter(|r| {
                            r.status.is_terminal()
                                && matches!(r.status, dog_queue::JobStatus::Completed { .. })
                                && r.attempt == 1
                        })
                        .count()
                }
                Err(e) => errors
                    .lock()
                    .unwrap()
                    .push(format!("snapshot {tenant}: {e}")),
            }
        }
    }
    let mut latencies = latency.lock().unwrap();
    latencies.sort_by(f64::total_cmp);
    let p95 = latencies
        .get((latencies.len() * 95 / 100).min(latencies.len().saturating_sub(1)))
        .copied();
    let errors = errors.lock().unwrap();
    let passed = accepted.load(Ordering::SeqCst) == offered
        && completed.load(Ordering::SeqCst) == offered
        && verified == offered
        && errors.is_empty()
        && overload.load(Ordering::SeqCst) == 0
        && late.load(Ordering::SeqCst) == 0
        && elapsed <= seconds as f64 + 5.0;
    println!(
        "{}",
        json!({"scope":"queue-level persistence and payload integrity; no HTTP or external payment effects","tenants":tenants,"jobs_per_second_per_tenant":10,"seconds":seconds,"payload_bytes":bytes,"workers_per_tenant":workers,"max_inflight_per_tenant":inflight,"shards":std::env::var("DOGRS_CAPACITY_SHARDS").unwrap_or_else(|_|"1".into()),"postgres_enqueue_concurrency":std::env::var("DOGRS_PG_ENQUEUE_CONCURRENCY").ok(),"postgres_pool_limit":std::env::var("DOGRS_PG_POOL_SIZE").unwrap_or_else(|_|"64".into()),"offered":offered,"accepted":accepted.load(Ordering::SeqCst),"completed":completed.load(Ordering::SeqCst),"verified_terminal_once":verified,"overload":overload.load(Ordering::SeqCst),"late_offers":late.load(Ordering::SeqCst),"elapsed_seconds":elapsed,"enqueue_p95_ms":p95,"error_count":errors.len(),"errors":errors.iter().take(10).collect::<Vec<_>>(),"passed":passed})
    );
    anyhow::ensure!(passed, "queue capacity gate failed");
    Ok(())
}
fn make_payload(seed: u64, size: usize) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let mut state = seed.wrapping_add(0x9e3779b97f4a7c15);
    for byte in &mut bytes[8..] {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
    bytes
}
