//! Matched open-loop admission diagnostics, not production queue acceptance.
//! Native-layout uses the same durable admission shape but omits queue behavior.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use dog_queue::{JobId, JobMessage, JobRecord, QueueBackend, QueueCtx};
use serde_json::json;
use tokio::sync::{mpsc, Semaphore};
use tokio_postgres::types::Type;
use tokio_postgres::Client;

use crate::runner::{env, tenant};

#[async_trait::async_trait]
pub trait Admission: Send + Sync {
    async fn put(&self, tenant: &str, n: usize, payload: Vec<u8>) -> Result<String>;
    async fn read(&self, tenant: &str, id: &str) -> Result<Vec<u8>>;
}
struct Dogrs<B>(Arc<B>);
#[async_trait::async_trait]
impl<B: QueueBackend + 'static> Admission for Dogrs<B> {
    async fn put(&self, tenant: &str, n: usize, payload: Vec<u8>) -> Result<String> {
        Ok(self
            .0
            .enqueue(QueueCtx::new(tenant), message(n, payload))
            .await?
            .to_string())
    }
    async fn read(&self, tenant: &str, id: &str) -> Result<Vec<u8>> {
        let record = self.0.get_record(QueueCtx::new(tenant), id.into()).await?;
        anyhow::ensure!(
            record.tenant_id == tenant && record.job_id.as_str() == id,
            "record identity mismatch"
        );
        Ok(record.message.payload_bytes)
    }
}
fn message(n: usize, payload: Vec<u8>) -> JobMessage {
    JobMessage::new("capacity", payload, "bytes", "capacity").with_idempotency_key(n.to_string())
}
pub async fn dogrs<B: QueueBackend + 'static>(backend: B) -> Result<()> {
    let backend = Arc::new(backend);
    let mut warmup = tokio::task::JoinSet::new();
    for _ in 0..64 {
        let backend = backend.clone();
        let tenant = format!("{}-warmup", tenant()?);
        warmup.spawn(async move {
            anyhow::ensure!(
                matches!(
                    backend
                        .get_status(
                            QueueCtx::new(tenant),
                            format!("{:064x}_{}", 0, JobId::new()).into()
                        )
                        .await,
                    Err(dog_queue::QueueError::JobNotFound(_))
                ),
                "unexpected pool warmup outcome"
            );
            Ok::<_, anyhow::Error>(())
        });
    }
    while let Some(result) = warmup.join_next().await {
        result??;
    }
    measure(Dogrs(backend), "dogrs-admission").await
}

struct Pg {
    clients: Vec<Client>,
    available: tokio::sync::Mutex<mpsc::UnboundedReceiver<usize>>,
    returned: mpsc::UnboundedSender<usize>,
    slots: Semaphore,
    layout: bool,
}
struct PgLease<'a> {
    index: usize,
    pool: &'a Pg,
}
impl Drop for PgLease<'_> {
    fn drop(&mut self) {
        let _ = self.pool.returned.send(self.index);
    }
}
impl Pg {
    async fn lease(&self) -> Result<PgLease<'_>> {
        let index = self
            .available
            .lock()
            .await
            .recv()
            .await
            .context("native pool closed")?;
        Ok(PgLease { index, pool: self })
    }
}
#[async_trait::async_trait]
impl Admission for Pg {
    async fn put(&self, tenant: &str, n: usize, payload: Vec<u8>) -> Result<String> {
        let _permit = self.slots.acquire().await?;
        let lease = self.lease().await?;
        let client = &self.clients[lease.index];
        let id = JobId::new();
        if self.layout {
            let message = message(n, Vec::new());
            let record = JobRecord::new(id.clone(), tenant, message.clone());
            let value = json!({"record":record,"token":null});
            let row = client
                .query_typed_one(
                    include_str!("../../../dog-queue/src/backend/postgres_enqueue.sql"),
                    &[
                        (&tenant, Type::TEXT),
                        (&id.as_str(), Type::TEXT),
                        (&value, Type::JSONB),
                        (&message.queue, Type::TEXT),
                        (&message.job_type, Type::TEXT),
                        (&message.idempotency_key, Type::TEXT),
                        (&i32::from(message.priority.as_u8()), Type::INT4),
                        (&message.run_at, Type::TIMESTAMPTZ),
                        (&payload, Type::BYTEA),
                    ],
                )
                .await?;
            Ok(row.get(0))
        } else {
            client
                .execute(
                    "INSERT INTO dogrs_native_payloads(tenant,id,payload) VALUES($1,$2,$3)",
                    &[&tenant, &id.as_str(), &payload],
                )
                .await?;
            Ok(id.to_string())
        }
    }
    async fn read(&self, tenant: &str, id: &str) -> Result<Vec<u8>> {
        let lease = self.lease().await?;
        let table = if self.layout {
            "dogrs_queue_jobs_v2"
        } else {
            "dogrs_native_payloads"
        };
        Ok(self.clients[lease.index]
            .query_one(
                &format!("SELECT payload FROM {table} WHERE tenant=$1 AND id=$2"),
                &[&tenant, &id],
            )
            .await?
            .get(0))
    }
}

#[cfg(feature = "nats")]
struct Nats {
    stores: Vec<(async_nats::jetstream::kv::Store, Semaphore)>,
    layout: bool,
}
#[cfg(feature = "nats")]
impl Nats {
    fn store(&self, tenant: &str) -> &(async_nats::jetstream::kv::Store, Semaphore) {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in tenant.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        &self.stores[(hash % self.stores.len() as u64) as usize]
    }
}
#[cfg(feature = "nats")]
fn hex(value: &str) -> String {
    value.bytes().map(|b| format!("{b:02x}")).collect()
}
#[cfg(feature = "nats")]
#[async_trait::async_trait]
impl Admission for Nats {
    async fn put(&self, tenant: &str, n: usize, payload: Vec<u8>) -> Result<String> {
        let (store, slots) = self.store(tenant);
        let _permit = slots.acquire().await?;
        // Equal key lengths/uniqueness; no dedupe races are part of this diagnostic.
        let slot = format!("{n:064x}");
        let id = format!("{slot}_{}", JobId::new());
        store
            .create(format!("p.{}.{id}", hex(tenant)), payload.into())
            .await?;
        if self.layout {
            let record = JobRecord::new(id.clone().into(), tenant, message(n, Vec::new()));
            let value = serde_json::to_vec(&json!({"record":record,"token":null}))?;
            store
                .update(format!("a.{}.{slot}", hex(tenant)), value.into(), 0)
                .await?;
        }
        Ok(id)
    }
    async fn read(&self, tenant: &str, id: &str) -> Result<Vec<u8>> {
        Ok(self
            .store(tenant)
            .0
            .get(format!("p.{}.{id}", hex(tenant)))
            .await?
            .context("missing native payload")?
            .to_vec())
    }
}

pub async fn native() -> Result<()> {
    let mode = env("DOGRS_ADMISSION_MODE")?;
    anyhow::ensure!(
        matches!(mode.as_str(), "native-payload" | "native-layout"),
        "invalid native diagnostic mode"
    );
    let layout = mode == "native-layout";
    match env("DOGRS_BACKEND")?.as_str() {
        "postgres" => {
            let uri = env("DOGRS_POSTGRES_URL")?;
            anyhow::ensure!(uri.contains("127.0.0.1"), "disposable loopback only");
            let (returned, available) = mpsc::unbounded_channel();
            let mut clients = Vec::new();
            for index in 0..64 {
                let (client, connection) =
                    tokio_postgres::connect(&uri, tokio_postgres::NoTls).await?;
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                client.batch_execute("SET statement_timeout='10s'").await?;
                clients.push(client);
                returned.send(index)?;
            }
            clients[0].batch_execute(if layout {include_str!("../../../dog-queue/src/backend/postgres_schema.sql")} else {"CREATE TABLE dogrs_native_payloads(tenant TEXT,id TEXT,payload BYTEA,PRIMARY KEY(tenant,id))"}).await?;
            measure(
                Pg {
                    clients,
                    available: tokio::sync::Mutex::new(available),
                    returned,
                    slots: Semaphore::new(48),
                    layout,
                },
                &mode,
            )
            .await
        }
        #[cfg(feature = "nats")]
        "nats" => {
            let uri = env("DOGRS_NATS_URL")?;
            anyhow::ensure!(uri.contains("127.0.0.1"), "disposable loopback only");
            let js = async_nats::jetstream::new(
                async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?,
            );
            let connections =
                std::env::var("DOGRS_NATS_CONNECTIONS").unwrap_or_else(|_| "shared".into());
            anyhow::ensure!(
                matches!(connections.as_str(), "shared" | "per-shard"),
                "NATS connections must be shared or per-shard"
            );
            let mut stores = Vec::new();
            for shard in 0..16 {
                let js = if connections == "per-shard" && shard > 0 {
                    async_nats::jetstream::new(
                        async_nats::connect(uri.split(',').collect::<Vec<_>>()).await?,
                    )
                } else {
                    js.clone()
                };
                let bucket = crate::connections::create_fixture_bucket(
                    &js,
                    async_nats::jetstream::kv::Config {
                        bucket: format!("{}_{shard}", env("DOGRS_NATS_BUCKET")?),
                        num_replicas: 3,
                        history: 1,
                        storage: async_nats::jetstream::stream::StorageType::File,
                        ..Default::default()
                    },
                )
                .await?;
                let mut config = bucket.stream.cached_info().config.clone();
                config.allow_direct = false;
                js.update_stream(config).await?;
                stores.push((js.get_key_value(&bucket.name).await?, Semaphore::new(16)));
            }
            measure(Nats { stores, layout }, &mode).await
        }
        _ => bail!("native admission diagnostic supports PostgreSQL and NATS"),
    }
}
async fn measure<B: Admission + 'static>(backend: B, mode: &str) -> Result<()> {
    let seconds: usize = env("DOGRS_CAPACITY_SECONDS")?.parse()?;
    let bytes: usize = env("DOGRS_CAPACITY_BYTES")?.parse()?;
    anyhow::ensure!(
        (1..=60).contains(&seconds) && (16..=65536).contains(&bytes),
        "invalid diagnostic bounds"
    );
    let backend = Arc::new(backend);
    let prefix = tenant()?;
    let started = Instant::now() + Duration::from_secs(1);
    let accepted = Arc::new(Mutex::new(Vec::new()));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let drops = Arc::new(AtomicUsize::new(0));
    let late = Arc::new(AtomicUsize::new(0));
    let latency = Arc::new(Mutex::new(Vec::new()));
    let mut producers = tokio::task::JoinSet::new();
    let timeout = Duration::from_secs(if env("DOGRS_BACKEND")? == "postgres" {
        10
    } else {
        30
    });
    for t in 0..100 {
        let backend = backend.clone();
        let tenant = format!("{prefix}-{t}");
        let accepted = accepted.clone();
        let errors = errors.clone();
        let drops = drops.clone();
        let late = late.clone();
        let latency = latency.clone();
        producers.spawn(async move {
            let slots = Arc::new(Semaphore::new(32));
            let mut requests = tokio::task::JoinSet::new();
            for n in 0..seconds * 10 {
                let due = started + Duration::from_millis((n * 100 + t) as u64);
                tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await;
                if Instant::now().saturating_duration_since(due) > Duration::from_millis(100) {
                    late.fetch_add(1, Ordering::Relaxed);
                }
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    drops.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                let backend = backend.clone();
                let tenant = tenant.clone();
                let accepted = accepted.clone();
                let errors = errors.clone();
                let latency = latency.clone();
                requests.spawn(async move {
                    let _permit = permit;
                    let begin = Instant::now();
                    let seed = ((t as u64) << 32) | n as u64;
                    match tokio::time::timeout(
                        timeout,
                        backend.put(&tenant, n, super::capacity::make_payload(seed, bytes)),
                    )
                    .await
                    {
                        Ok(Ok(id)) => accepted.lock().unwrap().push((tenant, seed, id)),
                        outcome => errors.lock().unwrap().push(format!("{outcome:?}")),
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
    let elapsed = started.elapsed().as_secs_f64();
    let accepted = Arc::try_unwrap(accepted).unwrap().into_inner().unwrap();
    let admitted = accepted.len();
    let mut verified = 0;
    let mut reads = tokio::task::JoinSet::new();
    for (tenant, seed, id) in accepted {
        if reads.len() >= 32 {
            reads.join_next().await.unwrap()??;
            verified += 1;
        }
        let backend = backend.clone();
        reads.spawn(async move {
            let payload = backend.read(&tenant, &id).await?;
            anyhow::ensure!(
                payload == super::capacity::make_payload(seed, bytes),
                "payload mismatch"
            );
            Ok::<_, anyhow::Error>(())
        });
    }
    while let Some(result) = reads.join_next().await {
        result??;
        verified += 1;
    }
    let errors = errors.lock().unwrap();
    let mut latency = latency.lock().unwrap();
    latency.sort_by(f64::total_cmp);
    let p95 = latency.get(latency.len() * 95 / 100).copied();
    let offered = seconds * 1000;
    let met = admitted == offered
        && verified == offered
        && errors.is_empty()
        && drops.load(Ordering::Relaxed) == 0
        && late.load(Ordering::Relaxed) == 0
        && elapsed <= seconds as f64 + 5.0;
    println!(
        "{}",
        json!({"measurement":"admission diagnostic only; no claims/completions/recovery certification","mode":mode,"backend":env("DOGRS_BACKEND")?,"offered":offered,"accepted":admitted,"verified_payloads":verified,"overload":drops.load(Ordering::Relaxed),"late_offers":late.load(Ordering::Relaxed),"seconds":seconds,"payload_bytes":bytes,"tenants":100,"jobs_per_second_per_tenant":10,"max_inflight_per_tenant":32,"elapsed_seconds":elapsed,"enqueue_p95_ms":p95,"error_count":errors.len(),"errors":errors.iter().take(3).collect::<Vec<_>>(),"admission_target_met":met,"production_acceptance":false})
    );
    // A diagnostic target miss remains a failing process; it is never a queue pass.
    anyhow::ensure!(met, "admission diagnostic target missed");
    Ok(())
}
