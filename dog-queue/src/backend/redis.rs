//! Indexed per-job Redis ledger. Enable AOF and configure replication for durability.
use super::durable::{DurableBackend, Operation, Outcome, StateStore, StoredRecord, TenantState};
use crate::{JobId, JobStatus, QueueError, QueueResult};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use redis::{aio::ConnectionManager, AsyncCommands};
use std::collections::HashMap;

#[derive(Clone)]
pub struct RedisConfig {
    pub connection_string: String,
}
pub struct RedisStore {
    manager: ConnectionManager,
    producers: ConnectionManager,
    checked: dashmap::DashSet<String>,
}
pub type RedisBackend = DurableBackend<RedisStore>;
fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
const TENANTS: &str = "dogrs-queue-v2:tenants";
fn hex(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn prefix(tenant: &str) -> String {
    format!("{{dogrs-queue-v2:{}}}", hex(tenant))
}
fn dedupe(row: &StoredRecord) -> String {
    row.record
        .message
        .idempotency_key
        .as_ref()
        .map(|key| {
            serde_json::to_string(&(&row.record.message.queue, &row.record.message.job_type, key))
                .unwrap()
        })
        .unwrap_or_default()
}
fn eligible(row: &StoredRecord) -> DateTime<Utc> {
    match row.record.status {
        JobStatus::Retrying { retry_at } => retry_at.max(row.record.message.run_at),
        _ => row.record.message.run_at,
    }
}
/// Deployment checks are backend-specific; the portable queue API stays unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub enum RedisDurability {
    /// Compatibility/development mode. Does not certify persistence.
    #[default]
    Unchecked,
    /// Fail startup unless AOF uses fsync=always and eviction is disabled.
    RequireAofAlways,
}
impl RedisBackend {
    pub async fn new_with_durability(
        config: RedisConfig,
        durability: RedisDurability,
    ) -> QueueResult<Self> {
        let backend = Self::new(config).await?;
        if matches!(durability, RedisDurability::RequireAofAlways) {
            backend.verify_persistence().await?;
        }
        Ok(backend)
    }
    /// Re-run after deployment/configuration changes. This checks the connected
    /// server, not replica synchronization or a managed provider's failover SLA.
    pub async fn verify_persistence(&self) -> QueueResult<()> {
        let mut connection = self.store.manager.clone();
        let info: String = redis::cmd("INFO")
            .arg("persistence")
            .query_async(&mut connection)
            .await
            .map_err(error)?;
        if !info.lines().any(|line| line.trim() == "aof_enabled:1") {
            return Err(QueueError::InvalidConfig(
                "Redis durable mode requires AOF; this server has it disabled".into(),
            ));
        }
        let config: HashMap<String, String> = redis::cmd("CONFIG")
            .arg("GET")
            .arg("appendfsync")
            .arg("maxmemory-policy")
            .query_async(&mut connection)
            .await
            .map_err(error)?;
        if config.get("appendfsync").map(String::as_str) != Some("always")
            || config.get("maxmemory-policy").map(String::as_str) != Some("noeviction")
        {
            return Err(QueueError::InvalidConfig(
                "Redis durable mode requires appendfsync=always and maxmemory-policy=noeviction"
                    .into(),
            ));
        }
        Ok(())
    }
    pub async fn new(config: RedisConfig) -> QueueResult<Self> {
        let client = redis::Client::open(config.connection_string).map_err(error)?;
        // Payload writes must not queue ahead of lease/completion traffic on
        // the same multiplexed socket. Both connections use identical bounded
        // reconnect/response policies; persistence and CAS rules are unchanged.
        let connection_config = redis::aio::ConnectionManagerConfig::new()
            .set_number_of_retries(3)
            .set_max_delay(500)
            .set_connection_timeout(std::time::Duration::from_secs(5))
            .set_response_timeout(std::time::Duration::from_secs(10));
        let manager = client
            .get_connection_manager_with_config(connection_config.clone())
            .await
            .map_err(error)?;
        let producers = client
            .get_connection_manager_with_config(connection_config)
            .await
            .map_err(error)?;
        Ok(Self {
            store: RedisStore {
                manager,
                producers,
                checked: Default::default(),
            },
            lease_duration: std::time::Duration::from_secs(300),
        })
    }
}
impl RedisStore {
    async fn check_legacy(&self, tenant: &str) -> QueueResult<()> {
        if !self.checked.contains(tenant) {
            let exists: bool = self
                .manager
                .clone()
                .exists(format!("{{dogrs-queue-v1}}:state:{}", hex(tenant)))
                .await
                .map_err(error)?;
            if exists {
                return Err(QueueError::InvalidConfig("Legacy Redis tenant detected: drain/export it with the previous release before selecting a fresh v2 tenant; never run legacy writers against a migrated tenant".into()));
            }
            self.checked.insert(tenant.into());
        }
        Ok(())
    }
    async fn write(
        &self,
        tenant: &str,
        stored: &StoredRecord,
        previous: &str,
        enqueue: bool,
        lease: i64,
        payload: &[u8],
        return_payload: bool,
    ) -> QueueResult<(String, Option<Vec<u8>>)> {
        let p = prefix(tenant);
        let r = &stored.record;
        // Do not build a JSON number array for the binary payload only to discard
        // it; that allocation dominates large-message admission.
        let mut record = stored.record.clone();
        record.message.payload_bytes.clear();
        let mut metadata = serde_json::to_value(StoredRecord {
            record,
            token: stored.token.clone(),
        })
        .map_err(error)?;
        metadata["redis_eligible"] = eligible(stored).timestamp_millis().into();
        metadata["redis_score"] = (r.created_at.timestamp_millis()
            - i64::from(r.message.priority.as_u8()) * 10_000_000_000_000)
            .into();
        let q = hex(&r.message.queue);
        let kind = if r.status.is_terminal() {
            "terminal"
        } else if r.status.is_processing() {
            "processing"
        } else {
            "ready"
        };
        let mut connection = if enqueue {
            self.producers.clone()
        } else {
            self.manager.clone()
        };
        redis::Script::new(include_str!("redis_write.lua"))
            .key(format!("{p}:meta"))
            .key(format!("{p}:payload"))
            .key(format!("{p}:pending:{q}"))
            .key(format!("{p}:ready:{q}"))
            .key(format!("{p}:leases"))
            .key(format!("{p}:terminal"))
            .key(format!("{p}:dedupe"))
            .arg(r.job_id.as_str())
            .arg(previous)
            .arg(if enqueue { "enqueue" } else { "update" })
            .arg(serde_json::to_string(&metadata).map_err(error)?)
            .arg(dedupe(stored))
            .arg(lease)
            .arg(payload)
            .arg(kind)
            .arg(r.lease_until().map(|d| d.timestamp_millis()).unwrap_or(0))
            .arg(r.updated_at.timestamp_millis())
            .arg(if return_payload { "payload" } else { "" })
            .invoke_async(&mut connection)
            .await
            .map_err(error)
    }
    async fn payload(&self, tenant: &str, id: &JobId) -> QueueResult<Vec<u8>> {
        self.manager
            .clone()
            .hget::<_, _, Option<Vec<u8>>>(format!("{}:payload", prefix(tenant)), id.as_str())
            .await
            .map_err(error)?
            .ok_or_else(|| error("Redis job payload missing; storage was evicted or lost"))
    }
}
impl RedisStore {
    async fn update_inner(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        self.check_legacy(tenant).await?;
        let p = prefix(tenant);
        if let Operation::Purge(before) = op {
            let count:usize=redis::Script::new("local ids=redis.call('ZRANGEBYSCORE',KEYS[3],'-inf','('..ARGV[1],'LIMIT',0,1000); for _,id in ipairs(ids) do redis.call('HDEL',KEYS[1],id); redis.call('HDEL',KEYS[2],id); redis.call('ZREM',KEYS[3],id); end; return #ids")
                .key(format!("{p}:meta")).key(format!("{p}:payload")).key(format!("{p}:terminal")).arg(before.timestamp_millis()).invoke_async(&mut self.manager.clone()).await.map_err(error)?;
            return Ok(Outcome::Purged(count));
        }
        let mut reaped = Vec::new();
        for _ in 0..64 {
            let script = redis::Script::new(include_str!("redis_read.lua"));
            let mut read = script.prepare_invoke();
            read.key(format!("{p}:meta")).key(format!("{p}:leases"));
            match op {
                Operation::Dequeue(queues, _) => {
                    read.arg("claim");
                    for queue in queues {
                        let q = hex(queue);
                        read.key(format!("{p}:pending:{q}"))
                            .key(format!("{p}:ready:{q}"));
                    }
                }
                Operation::Reap => {
                    read.arg("reap");
                }
                Operation::Snapshots(ids) => {
                    read.arg("read");
                    for id in ids {
                        read.arg(id.as_str());
                    }
                }
                Operation::Get(id)
                | Operation::Snapshot(id)
                | Operation::Cancel(id)
                | Operation::Complete(id, ..)
                | Operation::Fail(id, ..)
                | Operation::Heartbeat(id, ..) => {
                    read.arg("read").arg(id.as_str());
                }
                _ => {
                    read.arg("read");
                }
            }
            // Enqueue needs only the authoritative server clock, not existing
            // record metadata. Pipeline tenant registration and TIME instead of
            // waiting for registration before making a second empty read call.
            // The write/CAS still follows the acknowledged registration, so
            // failed registration cannot leave an undiscoverable accepted job.
            let (now, values) = if matches!(op, Operation::Enqueue(_)) {
                let (clock,): (Vec<i64>,) = redis::pipe()
                    .cmd("SADD")
                    .arg(TENANTS)
                    .arg(tenant)
                    .ignore()
                    .cmd("TIME")
                    .query_async(&mut self.producers.clone())
                    .await
                    .map_err(error)?;
                if clock.len() != 2 {
                    return Err(error("Invalid Redis clock"));
                }
                let now = DateTime::from_timestamp_millis(clock[0] * 1000 + clock[1] / 1000)
                    .ok_or_else(|| error("Invalid Redis clock"))?;
                (now, Vec::new())
            } else {
                let mut values: Vec<String> = read
                    .invoke_async(&mut self.manager.clone())
                    .await
                    .map_err(error)?;
                let now = DateTime::from_timestamp_millis(values.remove(0).parse().map_err(error)?)
                    .ok_or_else(|| error("Invalid Redis clock"))?;
                (now, values)
            };
            let mut state = TenantState::default();
            let mut previous = HashMap::new();
            for raw in values.into_iter().filter(|s| !s.is_empty()) {
                let row: StoredRecord = serde_json::from_str(&raw).map_err(error)?;
                previous.insert(row.record.job_id.clone(), raw);
                state.jobs.insert(row.record.job_id.clone(), row);
            }
            let metadata_op = if let Operation::Enqueue(message) = op {
                Some(Operation::Enqueue(super::durable::metadata_message(
                    message,
                )))
            } else {
                None
            };
            let mut outcome = state.apply_at(tenant, metadata_op.as_ref().unwrap_or(op), now)?;
            match &mut outcome {
                Outcome::Record(row) => {
                    row.message.payload_bytes = self.payload(tenant, &row.job_id).await?;
                    return Ok(outcome);
                }
                Outcome::Snapshot(_)
                | Outcome::Snapshots(_)
                | Outcome::Lease(None)
                | Outcome::Canceled(false) => return Ok(outcome),
                _ => {}
            }
            // Dequeue only modifies the winning record, not other queue heads.
            let selected = match &outcome {
                Outcome::Lease(Some(job)) => Some(job.record.job_id.clone()),
                _ => None,
            };
            let mut conflict = false;
            let mut claimed_payload = None;
            let mut committed = std::collections::HashSet::new();
            for (id, row) in &state.jobs {
                if selected.as_ref().is_some_and(|selected| selected != id) {
                    continue;
                }
                let old = previous.get(id).map(String::as_str).unwrap_or("");
                let lease = match op {
                    Operation::Complete(..) | Operation::Fail(..) | Operation::Heartbeat(..) => {
                        let previous: StoredRecord = serde_json::from_str(old).map_err(error)?;
                        previous
                            .record
                            .lease_until()
                            .map(|d| d.timestamp_millis())
                            .unwrap_or(0)
                    }
                    _ => 0,
                };
                let payload = if let Operation::Enqueue(message) = op {
                    message.payload_bytes.as_slice()
                } else {
                    &[]
                };
                let changed = self
                    .write(
                        tenant,
                        row,
                        old,
                        matches!(op, Operation::Enqueue(_)),
                        lease,
                        payload,
                        selected.is_some(),
                    )
                    .await?;
                if changed.0.is_empty() {
                    conflict = true;
                    break;
                }
                if matches!(op, Operation::Enqueue(_)) {
                    return Ok(Outcome::Id(changed.0.into()));
                }
                if selected.is_some() {
                    claimed_payload = changed.1;
                }
                committed.insert(id.clone());
            }
            if let Outcome::Reaped(rows) = &mut outcome {
                reaped.extend(rows.drain(..).filter(|row| committed.contains(&row.job_id)));
            }
            if conflict {
                tokio::task::yield_now().await;
                continue;
            }
            if let Outcome::Lease(Some(job)) = &mut outcome {
                job.record.message.payload_bytes = claimed_payload.ok_or_else(|| {
                    error("Redis job payload missing; storage was evicted or lost")
                })?;
            }
            if matches!(op, Operation::Reap) {
                return Ok(Outcome::Reaped(reaped));
            }
            return Ok(outcome);
        }
        if matches!(op, Operation::Dequeue(..)) {
            // Every attempted claim lost its CAS: this caller owns no lease.
            // Other workers making progress is an empty poll, not a backend
            // failure. Keep the work bounded and let the caller poll again.
            return Ok(Outcome::Lease(None));
        }
        Err(error("Redis job contention: retry operation"))
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        self.manager.clone().smembers(TENANTS).await.map_err(error)
    }
}

#[async_trait]
impl StateStore for RedisStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        tokio::time::timeout(std::time::Duration::from_secs(30),self.update_inner(tenant,op)).await.map_err(|_|error("Redis queue operation timed out; commit outcome may be unknown; use idempotency keys"))?
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            RedisStore::tenants(self),
        )
        .await
        .map_err(|_| error("Redis tenant lookup timed out"))?
    }
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use crate::{JobMessage, QueueBackend, QueueCtx};
    use redis::IntoConnectionInfo;

    #[tokio::test]
    #[ignore = "requires disposable Redis with ACL administration"]
    async fn redis_registration_failure_cannot_accept_a_job() {
        let url = std::env::var("DOGRS_REDIS_URL").unwrap();
        let mut backend = RedisBackend::new(RedisConfig {
            connection_string: url.clone(),
        })
        .await
        .unwrap();
        let user = format!("dogrs_test_{}", uuid::Uuid::new_v4().simple());
        let mut admin = backend.store.manager.clone();
        let _: () = redis::cmd("ACL")
            .arg("SETUSER")
            .arg(&user)
            .arg("on")
            .arg("nopass")
            .arg("~*")
            .arg("+@all")
            .arg("-sadd")
            .query_async(&mut admin)
            .await
            .unwrap();
        let mut info = url.into_connection_info().unwrap();
        info.redis.username = Some(user.clone());
        info.redis.password = Some(String::new());
        backend.store.producers = redis::Client::open(info)
            .unwrap()
            .get_connection_manager()
            .await
            .unwrap();
        let tenant = format!("registration-{}", uuid::Uuid::new_v4());
        let result = backend
            .enqueue(
                QueueCtx::new(&tenant),
                JobMessage::new("test", vec![1; 65536], "bytes", "q"),
            )
            .await;
        let exists: bool = admin
            .exists(format!("{}:meta", prefix(&tenant)))
            .await
            .unwrap();
        let _: usize = redis::cmd("ACL")
            .arg("DELUSER")
            .arg(&user)
            .query_async(&mut admin)
            .await
            .unwrap();
        assert!(result.is_err(), "failed registration must fail admission");
        assert!(
            !exists,
            "do not commit a job after a pipeline registration error"
        );
    }
}
