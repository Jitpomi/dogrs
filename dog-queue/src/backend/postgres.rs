//! Optional PostgreSQL ledger: indexed per-job rows, bounded pooling, server time.
use super::durable::{DurableBackend, Operation, Outcome, StateStore, StoredRecord, TenantState};
use crate::{JobStatus, QueueError, QueueResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
use tokio_postgres::{types::Type, Client, NoTls, Transaction};

#[derive(Clone)]
pub struct PostgresConfig {
    pub connection_string: String,
}
/// PostgreSQL-specific tuning. It is not part of the portable queue contract.
#[derive(Clone)]
pub struct PostgresOptions {
    pub max_connections: u32,
    pub operation_timeout: Duration,
    /// Explicit offline v1 -> v2 migration. Stop every old worker/API first.
    pub migrate_legacy: bool,
}
impl Default for PostgresOptions {
    fn default() -> Self {
        Self {
            max_connections: 4,
            operation_timeout: Duration::from_secs(10),
            migrate_legacy: false,
        }
    }
}
type Connector =
    Arc<dyn Fn() -> futures::future::BoxFuture<'static, QueueResult<Client>> + Send + Sync>;
#[derive(Clone)]
struct Manager(Connector);
impl bb8::ManageConnection for Manager {
    type Connection = Client;
    type Error = QueueError;
    async fn connect(&self) -> QueueResult<Client> {
        (self.0)().await
    }
    async fn is_valid(&self, client: &mut Client) -> QueueResult<()> {
        if client.is_closed() {
            Err(error("PostgreSQL connection closed"))
        } else {
            Ok(())
        }
    }
    fn has_broken(&self, client: &mut Client) -> bool {
        client.is_closed()
    }
}
pub struct PostgresStore {
    pool: bb8::Pool<Manager>,
    timeout: Duration,
}
pub type PostgresBackend = DurableBackend<PostgresStore>;
fn error(e: impl std::fmt::Display + 'static) -> QueueError {
    let message = (&e as &dyn std::any::Any)
        .downcast_ref::<tokio_postgres::Error>()
        .and_then(|e| e.as_db_error())
        .map(|db| format!("PostgreSQL {}: {}", db.code().code(), db.message()))
        .unwrap_or_else(|| e.to_string());
    QueueError::Internal(message)
}

impl PostgresBackend {
    /// Use only for a local database or trusted encrypted tunnel.
    pub async fn new(config: PostgresConfig) -> QueueResult<Self> {
        Self::new_with_tls(config, NoTls).await
    }
    pub async fn new_with_tls<T>(config: PostgresConfig, tls: T) -> QueueResult<Self>
    where
        T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>
            + Clone
            + Send
            + Sync
            + 'static,
        T::Stream: Send + 'static,
        T::TlsConnect: Send,
        <T::TlsConnect as tokio_postgres::tls::TlsConnect<tokio_postgres::Socket>>::Future: Send,
    {
        Self::new_with_tls_options(config, tls, PostgresOptions::default()).await
    }
    /// TLS remains caller-selected; migration is explicit and transactional.
    pub async fn new_with_tls_options<T>(
        config: PostgresConfig,
        tls: T,
        options: PostgresOptions,
    ) -> QueueResult<Self>
    where
        T: tokio_postgres::tls::MakeTlsConnect<tokio_postgres::Socket>
            + Clone
            + Send
            + Sync
            + 'static,
        T::Stream: Send + 'static,
        T::TlsConnect: Send,
        <T::TlsConnect as tokio_postgres::tls::TlsConnect<tokio_postgres::Socket>>::Future: Send,
    {
        if options.max_connections == 0 || options.operation_timeout.is_zero() {
            return Err(QueueError::InvalidConfig(
                "positive pool size and operation timeout required".into(),
            ));
        }
        let statement_timeout = options
            .operation_timeout
            .as_millis()
            .min(i32::MAX as u128)
            .max(1)
            .to_string();
        let connect: Connector = Arc::new(move || {
            let uri = config.connection_string.clone();
            let tls = tls.clone();
            let statement_timeout = statement_timeout.clone();
            Box::pin(async move {
                let (client, connection) =
                    tokio_postgres::connect(&uri, tls).await.map_err(error)?;
                tokio::spawn(async move {
                    if connection.await.is_err() {
                        tracing::warn!("PostgreSQL queue connection closed");
                    }
                });
                client
                    .query_one(
                        "SELECT set_config('statement_timeout', $1, false)",
                        &[&statement_timeout],
                    )
                    .await
                    .map_err(error)?;
                Ok(client)
            })
        });
        let pool = bb8::Pool::builder()
            .max_size(options.max_connections)
            .connection_timeout(options.operation_timeout)
            .retry_connection(false)
            .build(Manager(connect))
            .await
            .map_err(error)?;
        let store = PostgresStore {
            pool,
            timeout: options.operation_timeout,
        };
        tokio::time::timeout(store.timeout, store.initialize(options.migrate_legacy))
            .await
            .map_err(|_| error("PostgreSQL schema initialization timed out"))??;
        Ok(Self {
            store,
            lease_duration: Duration::from_secs(300),
        })
    }
}

async fn schema_ready(client: &impl tokio_postgres::GenericClient) -> QueueResult<bool> {
    let present: bool = client.query_one("SELECT to_regclass('dogrs_queue_metadata_v2') IS NOT NULL AND EXISTS (SELECT 1 FROM pg_attribute WHERE attrelid=to_regclass('dogrs_queue_jobs_v2') AND attname='payload' AND NOT attisdropped)", &[]).await.map_err(error)?.get(0);
    if !present {
        return Ok(false);
    }
    Ok(client
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM dogrs_queue_metadata_v2 WHERE version=2)",
            &[],
        )
        .await
        .map_err(error)?
        .get(0))
}

impl PostgresStore {
    async fn initialize(&self, migrate: bool) -> QueueResult<()> {
        let mut client = self.pool.get().await.map_err(error)?;
        if schema_ready(&*client).await? {
            return Ok(());
        }
        let tx = client.transaction().await.map_err(error)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtext('dogrs_queue_schema_v1')::bigint)",
            &[],
        )
        .await
        .map_err(error)?;
        if schema_ready(&tx).await? {
            return tx.commit().await.map_err(error);
        }
        tx.batch_execute(include_str!("postgres_schema.sql"))
            .await
            .map_err(error)?;
        let done: bool = tx
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM dogrs_queue_metadata_v2 WHERE version=2)",
                &[],
            )
            .await
            .map_err(error)?
            .get(0);
        if !done {
            tx.batch_execute("LOCK TABLE dogrs_queue_state_v1 IN ACCESS EXCLUSIVE MODE")
                .await
                .map_err(error)?;
            let rows = tx
                .query("SELECT tenant,state FROM dogrs_queue_state_v1", &[])
                .await
                .map_err(error)?;
            if !migrate
                && rows.iter().any(|r| {
                    r.get::<_, serde_json::Value>(1)["jobs"]
                        .as_object()
                        .is_some_and(|j| !j.is_empty())
                })
            {
                return Err(QueueError::InvalidConfig("Legacy PostgreSQL queue detected: stop old workers, back up the database, then explicitly set PostgresOptions.migrate_legacy=true".into()));
            }
            for row in rows {
                let tenant: String = row.get(0);
                let state: TenantState = serde_json::from_value(row.get(1)).map_err(error)?;
                for (id, stored) in &state.jobs {
                    if stored.record.tenant_id != tenant || id != &stored.record.job_id {
                        return Err(error("legacy tenant mismatch"));
                    }
                    insert(&tx, stored).await?;
                }
            }
            // Fence old binaries instead of allowing two diverging ledgers. Keep
            // v1 data intact for offline rollback; never replay it on restart.
            tx.batch_execute("CREATE OR REPLACE FUNCTION dogrs_queue_v1_read_only() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'DogRS queue migrated to v2; stop legacy writers'; END $$; CREATE TRIGGER dogrs_queue_v1_fence BEFORE INSERT OR UPDATE OR DELETE OR TRUNCATE ON dogrs_queue_state_v1 FOR EACH STATEMENT EXECUTE FUNCTION dogrs_queue_v1_read_only(); INSERT INTO dogrs_queue_metadata_v2(version) VALUES (2)").await.map_err(error)?;
        }
        tx.commit().await.map_err(error)
    }
    async fn update_inner(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        let mut client = self.pool.get().await.map_err(error)?;
        if let Operation::Enqueue(message) = op {
            let mut state = TenantState::default();
            state.apply_at(tenant, op, Utc::now())?;
            let stored = state.jobs.values().next().unwrap();
            let r = &stored.record;
            let value = metadata(stored)?;
            // One atomic statement: database timestamps replace temporary local
            // constructor timestamps before anything becomes visible.
            let row = client
                .query_typed_one(
                    include_str!("postgres_enqueue.sql"),
                    &[
                        (&tenant, Type::TEXT),
                        (&r.job_id.as_str(), Type::TEXT),
                        (&value, Type::JSONB),
                        (&message.queue, Type::TEXT),
                        (&message.job_type, Type::TEXT),
                        (&message.idempotency_key, Type::TEXT),
                        (&i32::from(message.priority.as_u8()), Type::INT4),
                        (&message.run_at, Type::TIMESTAMPTZ),
                        (&message.payload_bytes, Type::BYTEA),
                    ],
                )
                .await
                .map_err(error)?;
            return Ok(Outcome::Id(row.get::<_, String>(0).into()));
        }
        if let Operation::Dequeue(queues, duration) = op {
            if duration.is_zero() || chrono::Duration::from_std(*duration).is_err() {
                return Err(QueueError::InvalidConfig(
                    "lease duration must be positive and representable".into(),
                ));
            }
            let token = crate::LeaseToken::new();
            let row = client
                .query_typed_opt(
                    include_str!("postgres_claim.sql"),
                    &[
                        (&tenant, Type::TEXT),
                        (queues, Type::TEXT_ARRAY),
                        (&token.as_str(), Type::TEXT),
                        (&duration.as_secs_f64(), Type::FLOAT8),
                    ],
                )
                .await
                .map_err(error)?;
            let Some(row) = row else {
                return Ok(Outcome::Lease(None));
            };
            let mut stored = decode_payload(&row)?;
            let until = stored
                .record
                .lease_until()
                .ok_or_else(|| error("claim did not return a lease"))?;
            stored.record.lease_token = Some(token.clone());
            return Ok(Outcome::Lease(Some(crate::LeasedJob::new(
                stored.record,
                token,
                until,
            ))));
        }
        if let Operation::Snapshots(ids) = op {
            let keys: Vec<&str> = ids.iter().map(|id| id.as_str()).collect();
            let rows = client
                .query_typed(
                    "SELECT state FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=ANY($2)",
                    &[(&tenant, Type::TEXT), (&keys, Type::TEXT_ARRAY)],
                )
                .await
                .map_err(error)?;
            let mut found = std::collections::HashMap::new();
            for row in rows {
                let stored: StoredRecord = serde_json::from_value(row.get(0)).map_err(error)?;
                found.insert(
                    stored.record.job_id.clone(),
                    crate::JobSnapshot::from(&stored.record),
                );
            }
            return Ok(Outcome::Snapshots(
                ids.iter()
                    .map(|id| {
                        found
                            .get(id)
                            .cloned()
                            .ok_or_else(|| QueueError::JobNotFound(id.clone()))
                    })
                    .collect::<QueueResult<_>>()?,
            ));
        }
        if let Operation::Snapshot(id) = op {
            let row = client
                .query_typed_opt(
                    "SELECT state FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2",
                    &[(&tenant, Type::TEXT), (&id.as_str(), Type::TEXT)],
                )
                .await
                .map_err(error)?
                .ok_or_else(|| QueueError::JobNotFound(id.clone()))?;
            let stored: StoredRecord = serde_json::from_value(row.get(0)).map_err(error)?;
            return Ok(Outcome::Snapshot(crate::JobSnapshot::from(&stored.record)));
        }
        if let Operation::Get(id) = op {
            let row = client
                .query_typed_opt(
                    "SELECT state,payload FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2",
                    &[(&tenant, Type::TEXT), (&id.as_str(), Type::TEXT)],
                )
                .await
                .map_err(error)?
                .ok_or_else(|| QueueError::JobNotFound(id.clone()))?;
            let stored = decode_payload(&row)?;
            return Ok(Outcome::Record(stored.record));
        }
        if let Operation::Purge(before) = op {
            let count = client.execute("DELETE FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND NOT active AND updated_at < $2", &[&tenant,before]).await.map_err(error)?;
            return Ok(Outcome::Purged(count as usize));
        }
        // A metadata read followed by a fenced compare-and-swap needs two round
        // trips rather than BEGIN/lock/time/update/COMMIT. The write checks the
        // database clock again: a lease that expired in transit cannot commit.
        if let Operation::Complete(id, ..)
        | Operation::Fail(id, ..)
        | Operation::Heartbeat(id, ..)
        | Operation::Cancel(id) = op
        {
            for _ in 0..32 {
                let row = client.query_typed_opt(
                    "SELECT state,clock_timestamp() FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2",
                    &[(&tenant,Type::TEXT),(&id.as_str(),Type::TEXT)]).await.map_err(error)?;
                let Some(row) = row else {
                    return if matches!(op, Operation::Cancel(_)) {
                        Ok(Outcome::Canceled(false))
                    } else {
                        Err(QueueError::JobNotFound(id.clone()))
                    };
                };
                let previous: serde_json::Value = row.get(0);
                let stored: StoredRecord =
                    serde_json::from_value(previous.clone()).map_err(error)?;
                let mut state = TenantState::default();
                state.jobs.insert(id.clone(), stored);
                let outcome = state.apply_at(tenant, op, row.get(1))?;
                if matches!(outcome, Outcome::Canceled(false)) {
                    return Ok(outcome);
                }
                let stored = &state.jobs[id];
                let r = &stored.record;
                let value = metadata(stored)?;
                let require_lease = !matches!(op, Operation::Cancel(_));
                let changed = client.query_typed_opt(
                    "WITH locked AS MATERIALIZED (SELECT id FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2 FOR UPDATE), stamped AS MATERIALIZED (SELECT id,clock_timestamp() AS now FROM locked) UPDATE dogrs_queue_jobs_v2 j SET state=$3,updated_at=$4,eligible_at=$5,lease_until=$6,status=$7,active=$8,payload=COALESCE(payload,$9) FROM stamped WHERE j.tenant=$1 AND j.id=stamped.id AND j.state=$10 AND (NOT $11 OR j.lease_until > stamped.now) RETURNING j.id",
                    &[(&tenant,Type::TEXT),(&id.as_str(),Type::TEXT),(&value,Type::JSONB),(&r.updated_at,Type::TIMESTAMPTZ),(&eligible(stored),Type::TIMESTAMPTZ),(&r.lease_until(),Type::TIMESTAMPTZ),(&r.status.name(),Type::TEXT),(&!r.status.is_terminal(),Type::BOOL),(&r.message.payload_bytes,Type::BYTEA),(&previous,Type::JSONB),(&require_lease,Type::BOOL)]
                ).await.map_err(error)?;
                if changed.is_some() {
                    return Ok(outcome);
                }
                tokio::task::yield_now().await;
            }
            return Err(error("PostgreSQL job contention: retry operation"));
        }
        let tx = client.transaction().await.map_err(error)?;
        let rows = match op {
            Operation::Reap => tx.query_typed("SELECT state FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND lease_until < statement_timestamp() ORDER BY lease_until LIMIT 256 FOR UPDATE SKIP LOCKED", &[(&tenant,Type::TEXT)]).await.map_err(error)?,
            Operation::Complete(id, ..) | Operation::Fail(id, ..) | Operation::Heartbeat(id, ..) | Operation::Cancel(id) => tx.query_typed("SELECT state FROM dogrs_queue_jobs_v2 WHERE tenant=$1 AND id=$2 FOR UPDATE", &[(&tenant,Type::TEXT),(&id.as_str(),Type::TEXT)]).await.map_err(error)?,
            _ => unreachable!(),
        };
        // Read time after row locks are held, not before a potentially long wait.
        let now: DateTime<Utc> = tx
            .query_typed_one("SELECT clock_timestamp()", &[])
            .await
            .map_err(error)?
            .get(0);
        let mut state = TenantState::default();
        for row in rows {
            let stored: StoredRecord = serde_json::from_value(row.get(0)).map_err(error)?;
            state.jobs.insert(stored.record.job_id.clone(), stored);
        }
        let outcome = state.apply_at(tenant, op, now)?;
        for stored in state.jobs.values() {
            persist(&tx, stored).await?;
        }
        tx.commit().await.map_err(error)?;
        Ok(outcome)
    }
}
fn metadata(stored: &StoredRecord) -> QueueResult<serde_json::Value> {
    let mut record = stored.record.clone();
    record.message.payload_bytes.clear();
    serde_json::to_value(StoredRecord {
        record,
        token: stored.token.clone(),
    })
    .map_err(error)
}
fn decode_payload(row: &tokio_postgres::Row) -> QueueResult<StoredRecord> {
    let mut stored: StoredRecord = serde_json::from_value(row.get(0)).map_err(error)?;
    // Nullable payload supports the pre-binary development layout until its first
    // mutation backfills the binary column. Offline v1 migration writes it directly.
    if let Some(payload) = row.get::<_, Option<Vec<u8>>>(1) {
        stored.record.message.payload_bytes = payload;
    }
    Ok(stored)
}
fn eligible(stored: &StoredRecord) -> DateTime<Utc> {
    match stored.record.status {
        JobStatus::Retrying { retry_at } => retry_at.max(stored.record.message.run_at),
        _ => stored.record.message.run_at,
    }
}
async fn insert(tx: &Transaction<'_>, stored: &StoredRecord) -> QueueResult<String> {
    let r = &stored.record;
    let m = &r.message;
    let value = metadata(stored)?;
    let base = "INSERT INTO dogrs_queue_jobs_v2 (tenant,id,state,queue,kind,dedupe,priority,created_at,updated_at,eligible_at,lease_until,status,active,payload) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)";
    let sql = format!("{base} RETURNING id");
    Ok(tx
        .query_one(
            &sql,
            &[
                &r.tenant_id,
                &r.job_id.as_str(),
                &value,
                &m.queue,
                &m.job_type,
                &m.idempotency_key,
                &i32::from(m.priority.as_u8()),
                &r.created_at,
                &r.updated_at,
                &eligible(stored),
                &r.lease_until(),
                &r.status.name(),
                &!r.status.is_terminal(),
                &m.payload_bytes,
            ],
        )
        .await
        .map_err(error)?
        .get(0))
}
async fn persist(tx: &Transaction<'_>, stored: &StoredRecord) -> QueueResult<()> {
    let r = &stored.record;
    let value = metadata(stored)?;
    tx.query_typed("UPDATE dogrs_queue_jobs_v2 SET state=$3,updated_at=$4,eligible_at=$5,lease_until=$6,status=$7,active=$8,payload=COALESCE(payload,$9) WHERE tenant=$1 AND id=$2", &[(&r.tenant_id,Type::TEXT),(&r.job_id.as_str(),Type::TEXT),(&value,Type::JSONB),(&r.updated_at,Type::TIMESTAMPTZ),(&eligible(stored),Type::TIMESTAMPTZ),(&r.lease_until(),Type::TIMESTAMPTZ),(&r.status.name(),Type::TEXT),(&!r.status.is_terminal(),Type::BOOL),(&r.message.payload_bytes,Type::BYTEA)]).await.map_err(error)?;
    Ok(())
}
#[async_trait]
impl StateStore for PostgresStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        tokio::time::timeout(self.timeout, self.update_inner(tenant,op)).await.map_err(|_| error("PostgreSQL queue operation timed out; commit outcome may be unknown; use idempotency keys"))?
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        tokio::time::timeout(self.timeout, async {
            let client = self.pool.get().await.map_err(error)?;
            Ok(client.query_typed("SELECT DISTINCT tenant FROM dogrs_queue_jobs_v2 WHERE lease_until < statement_timestamp()", &[]).await.map_err(error)?.into_iter().map(|r|r.get(0)).collect())
        }).await.map_err(|_| error("PostgreSQL expiry query timed out"))?
    }
}
