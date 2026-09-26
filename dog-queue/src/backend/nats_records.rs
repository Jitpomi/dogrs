//! One CAS cell per active idempotency scope; immutable payload and terminal history
//! live outside the cell. A replayable watch is only a discovery hint: every claim
//! re-reads the authoritative cell and CAS-fences ownership.
use super::{
    durable::{Operation, Outcome, StateStore, StoredRecord, TenantState},
    nats::NatsStore,
};
use crate::{JobId, QueueError, QueueResult};
use async_nats::jetstream::{
    consumer::{push::OrderedConfig, DeliverPolicy, ReplayPolicy},
    kv,
};
use async_trait::async_trait;
use chrono::Utc;
use futures::{StreamExt, TryStreamExt};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};

fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
// NATS 2.11 may use code 10164; async-nats 0.50 only maps 10071 to
// WrongLastRevision. Inspect structured causes rather than matching error text.
fn revision_conflict(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(server) = error.downcast_ref::<async_nats::jetstream::Error>() {
            return matches!(
                server.error_code(),
                async_nats::jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE
                    | async_nats::jetstream::ErrorCode::STREAM_WRONG_LAST_SEQUENCE_CONSTANT
            );
        }
        cause = error.source();
    }
    false
}
fn hex(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}
fn cell(tenant: &str, slot: &str) -> String {
    format!("a.{}.{}", hex(tenant), slot)
}
fn payload(tenant: &str, id: &JobId) -> String {
    format!("p.{}.{}", hex(tenant), id)
}
fn history(tenant: &str, id: &JobId) -> String {
    format!("h.{}.{}", hex(tenant), id)
}
fn slot(id: &JobId) -> QueueResult<&str> {
    let Some((part, nonce)) = id.as_str().split_once('_') else {
        return Err(QueueError::JobNotFound(id.clone()));
    };
    if part.len() != 64
        || !part.bytes().all(|b| b.is_ascii_hexdigit())
        || uuid::Uuid::parse_str(nonce).is_err()
    {
        return Err(QueueError::JobNotFound(id.clone()));
    }
    Ok(part)
}
type TenantIndex = dashmap::DashMap<String, (u64, Result<StoredRecord, String>)>;
pub(super) struct Index {
    entries: dashmap::DashMap<String, Arc<TenantIndex>>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
impl Drop for Index {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().unwrap().take() {
            task.abort();
        }
    }
}
impl Index {
    fn tenant(&self, tenant: &str) -> Option<Arc<TenantIndex>> {
        self.entries
            .get(&hex(tenant))
            .map(|entry| entry.value().clone())
    }
    fn observe(&self, key: String, revision: u64, value: Vec<u8>, deleted: bool) {
        use dashmap::mapref::entry::Entry;
        // Decode once per observed revision, not on every poll by every worker.
        // Keep malformed metadata as an error so it cannot silently hide jobs.
        let parsed = || serde_json::from_slice(&value).map_err(|e| e.to_string());
        let Some((tenant, _)) = key.strip_prefix("a.").and_then(|s| s.split_once('.')) else {
            return;
        };
        let entries = self
            .entries
            .entry(tenant.into())
            .or_default()
            .value()
            .clone();
        match entries.entry(key) {
            Entry::Occupied(mut entry) => {
                if entry.get().0 <= revision {
                    if deleted {
                        let _ = entry.remove();
                    } else {
                        let _ = entry.insert((revision, parsed()));
                    }
                }
            }
            Entry::Vacant(entry) => {
                if !deleted {
                    let _ = entry.insert((revision, parsed()));
                }
            }
        };
    }
}
impl NatsStore {
    async fn index(&self) -> QueueResult<&Arc<Index>> {
        self.index
            .get_or_try_init(|| async {
                let index = Arc::new(Index {
                    entries: Default::default(),
                    task: Default::default(),
                });
                // Last-per-subject replay installs the initial snapshot before claims.
                let consumer = self
                    .bucket
                    .stream
                    .create_consumer(OrderedConfig {
                        deliver_subject: format!("_INBOX.{}", uuid::Uuid::new_v4().simple()),
                        filter_subject: format!("$KV.{}.a.>", self.bucket.name),
                        deliver_policy: DeliverPolicy::LastPerSubject,
                        replay_policy: ReplayPolicy::Instant,
                        ..Default::default()
                    })
                    .await
                    .map_err(error)?;
                let mut pending = consumer.cached_info().num_pending;
                let mut messages = consumer.messages().await.map_err(error)?;
                let prefix = format!("$KV.{}.", self.bucket.name);
                while pending > 0 {
                    let message = tokio::time::timeout(Duration::from_secs(15), messages.next())
                        .await
                        .map_err(error)?
                        .ok_or_else(|| error("JetStream index closed"))?
                        .map_err(error)?;
                    let info = message.info().map_err(error)?;
                    pending = info.pending;
                    let deleted = message
                        .headers
                        .as_ref()
                        .and_then(|h| h.get("KV-Operation"))
                        .is_some();
                    if let Some(key) = message.subject.strip_prefix(&prefix) {
                        index.observe(
                            key.into(),
                            info.stream_sequence,
                            message.payload.to_vec(),
                            deleted,
                        );
                    }
                }
                let weak = Arc::downgrade(&index);
                let bucket = self.bucket.clone();
                let task = tokio::spawn(async move {
                    loop {
                        match tokio::time::timeout(Duration::from_secs(60), messages.next()).await {
                            Ok(Some(Ok(message))) => {
                                let Some(index) = weak.upgrade() else { break };
                                if let Ok(info) = message.info() {
                                    let deleted = message
                                        .headers
                                        .as_ref()
                                        .and_then(|h| h.get("KV-Operation"))
                                        .is_some();
                                    if let Some(key) = message.subject.strip_prefix(&prefix) {
                                        index.observe(
                                            key.into(),
                                            info.stream_sequence,
                                            message.payload.to_vec(),
                                            deleted,
                                        );
                                    }
                                }
                                continue;
                            }
                            _ => {
                                tracing::debug!("Rebuilding JetStream queue discovery index");
                            }
                        }
                        // Recreate after disconnect/end/error, and periodically during
                        // idle periods. This is a replayable hint, never lease authority.
                        loop {
                            if weak.upgrade().is_none() {
                                return;
                            }
                            let result = async {
                                let consumer = bucket
                                    .stream
                                    .create_consumer(OrderedConfig {
                                        deliver_subject: format!(
                                            "_INBOX.{}",
                                            uuid::Uuid::new_v4().simple()
                                        ),
                                        filter_subject: format!("$KV.{}.a.>", bucket.name),
                                        deliver_policy: DeliverPolicy::LastPerSubject,
                                        replay_policy: ReplayPolicy::Instant,
                                        ..Default::default()
                                    })
                                    .await
                                    .map_err(error)?;
                                consumer.messages().await.map_err(error)
                            }
                            .await;
                            match result {
                                Ok(replay) => {
                                    if let Some(index) = weak.upgrade() {
                                        index.entries.clear();
                                    }
                                    messages = replay;
                                    break;
                                }
                                Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
                            }
                        }
                    }
                });
                *index.task.lock().unwrap() = Some(task);
                Ok(index)
            })
            .await
    }
    async fn legacy(&self, tenant: &str) -> QueueResult<()> {
        if self
            .bucket
            .entry(format!("tenant_{}", hex(tenant)))
            .await
            .map_err(error)?
            .is_some_and(|e| e.operation == kv::Operation::Put)
        {
            return Err(QueueError::InvalidConfig("Legacy JetStream tenant detected: drain/export with the previous release and select a fresh v2 tenant; stop legacy writers first".into()));
        }
        Ok(())
    }
    async fn read(&self, tenant: &str, id: &JobId) -> QueueResult<(String, kv::Entry)> {
        let key = cell(tenant, slot(id)?);
        if let Some(entry) = self.bucket.entry(&key).await.map_err(error)? {
            self.index().await?.observe(
                key.clone(),
                entry.revision,
                entry.value.to_vec(),
                entry.operation != kv::Operation::Put,
            );
            if entry.operation == kv::Operation::Put {
                let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
                if row.record.job_id == *id {
                    return Ok((key, entry));
                }
            }
        }
        let key = history(tenant, id);
        let entry = self
            .bucket
            .entry(&key)
            .await
            .map_err(error)?
            .filter(|e| e.operation == kv::Operation::Put);
        if let Some(entry) = entry {
            return Ok((key, entry));
        }
        Err(QueueError::JobNotFound(id.clone()))
    }
    async fn bytes(&self, tenant: &str, id: &JobId) -> QueueResult<Vec<u8>> {
        self.bucket
            .get(payload(tenant, id))
            .await
            .map_err(error)?
            .map(|b| b.to_vec())
            .ok_or_else(|| error("JetStream job payload missing; storage was lost"))
    }
    async fn archive(&self, tenant: &str, row: &StoredRecord) -> QueueResult<()> {
        let key = history(tenant, &row.record.job_id);
        let value = serde_json::to_vec(row).map_err(error)?;
        match self.bucket.create(&key, value.clone().into()).await {
            Ok(_) => Ok(()),
            Err(err) => match self.bucket.get(&key).await.map_err(error)? {
                Some(old) if old.as_ref() == value => Ok(()),
                _ => Err(error(err)),
            },
        }
    }
    async fn cas(&self, key: &str, value: Vec<u8>, revision: u64) -> QueueResult<bool> {
        match self
            .bucket
            .update(key, value.clone().into(), revision)
            .await
        {
            Ok(revision) => {
                self.index()
                    .await?
                    .observe(key.into(), revision, value, false);
                Ok(true)
            }
            Err(err)
                if err.kind() == kv::UpdateErrorKind::WrongLastRevision
                    || revision_conflict(&err) =>
            {
                Ok(false)
            }
            Err(err) => Err(error(err)),
        }
    }
    async fn retire(&self, tenant: &str, key: &str) -> QueueResult<()> {
        if let Some(entry) = self.bucket.entry(key).await.map_err(error)? {
            if entry.operation != kv::Operation::Put {
                return Ok(());
            }
            let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
            if row.record.status.is_terminal() {
                self.archive(tenant, &row).await?;
                // Deleting only this revision cannot remove a concurrent replacement.
                if let Err(err) = self
                    .bucket
                    .purge_expect_revision(key, Some(entry.revision))
                    .await
                {
                    // A replacement is benign; outages and other errors remain visible.
                    if self
                        .bucket
                        .entry(key)
                        .await
                        .map_err(error)?
                        .is_some_and(|e| e.revision == entry.revision)
                    {
                        return Err(error(err));
                    }
                }
            }
        }
        Ok(())
    }
}
impl NatsStore {
    async fn update_inner(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        self.legacy(tenant).await?;
        let index = self.index().await?;
        if let Operation::Enqueue(message) = op {
            let hash = if let Some(dedupe) = &message.idempotency_key {
                format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&(&message.queue, &message.job_type, dedupe))
                            .map_err(error)?
                    )
                )
            } else {
                format!("{:x}", Sha256::digest(uuid::Uuid::new_v4().as_bytes()))
            };
            let key = cell(tenant, &hash);
            let mut state = TenantState::default();
            state.apply_at(tenant, op, Utc::now())?;
            let (_, mut row) = state.jobs.into_iter().next().unwrap();
            let id = JobId::from(format!("{hash}_{}", uuid::Uuid::new_v4()));
            row.record.job_id = id.clone();
            row.record.message.payload_bytes.clear();
            let value = serde_json::to_vec(&row).map_err(error)?;
            if message.payload_bytes.len() > self.max_state_bytes
                || value.len() + 8192 > self.max_state_bytes
            {
                return Err(QueueError::InvalidConfig(
                    "JetStream job exceeds payload or metadata limit".into(),
                ));
            }
            // Immutable payload must be durable before a discoverable job is committed.
            self.bucket
                .create(payload(tenant, &id), message.payload_bytes.clone().into())
                .await
                .map_err(error)?;
            for _ in 0..64 {
                let previous = self.bucket.entry(&key).await.map_err(error)?;
                let revision = previous.as_ref().map(|e| e.revision).unwrap_or(0);
                if let Some(entry) = previous.filter(|e| e.operation == kv::Operation::Put) {
                    let existing: StoredRecord =
                        serde_json::from_slice(&entry.value).map_err(error)?;
                    if !existing.record.status.is_terminal() {
                        index.observe(key.clone(), entry.revision, entry.value.to_vec(), false);
                        self.bucket
                            .purge(payload(tenant, &id))
                            .await
                            .map_err(error)?;
                        return Ok(Outcome::Id(existing.record.job_id));
                    }
                    self.archive(tenant, &existing).await?;
                }
                if self.cas(&key, value.clone(), revision).await? {
                    return Ok(Outcome::Id(id));
                }
            }
            return Err(error("JetStream enqueue contention"));
        }
        if let Operation::Snapshots(ids) = op {
            let result = futures::stream::iter(ids.clone().into_iter().map(|id| async move {
                let (_, entry) = self.read(tenant, &id).await?;
                let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
                Ok::<_, QueueError>(crate::JobSnapshot::from(&row.record))
            }))
            .buffered(16)
            .try_collect::<Vec<_>>()
            .await?;
            return Ok(Outcome::Snapshots(result));
        }
        if let Operation::Purge(before) = op {
            let prefix = format!("h.{}.", hex(tenant));
            let mut keys = self.bucket.keys().await.map_err(error)?;
            let mut count = 0;
            while let Some(key) = keys.next().await {
                let key = key.map_err(error)?;
                if !key.starts_with(&prefix) {
                    continue;
                }
                if let Some(entry) = self
                    .bucket
                    .entry(&key)
                    .await
                    .map_err(error)?
                    .filter(|e| e.operation == kv::Operation::Put)
                {
                    let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
                    if row.record.updated_at < *before {
                        self.retire(tenant, &cell(tenant, slot(&row.record.job_id)?))
                            .await?;
                        self.bucket
                            .purge_expect_revision(&key, Some(entry.revision))
                            .await
                            .map_err(error)?;
                        self.bucket
                            .purge(payload(tenant, &row.record.job_id))
                            .await
                            .map_err(error)?;
                        count += 1;
                    }
                }
            }
            return Ok(Outcome::Purged(count));
        }
        if matches!(op, Operation::Reap) {
            let keys: Vec<String> = index
                .tenant(tenant)
                .map(|entries| entries.iter().map(|entry| entry.key().clone()).collect())
                .unwrap_or_default();
            let mut outcomes = Vec::new();
            for key in keys {
                for _ in 0..16 {
                    let Some(entry) = self
                        .bucket
                        .entry(&key)
                        .await
                        .map_err(error)?
                        .filter(|e| e.operation == kv::Operation::Put)
                    else {
                        break;
                    };
                    let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
                    if row.record.status.is_terminal() {
                        self.retire(tenant, &key).await?;
                        break;
                    }
                    if !row.record.lease_expired(Utc::now()) {
                        break;
                    }
                    let mut state = TenantState::default();
                    state.jobs.insert(row.record.job_id.clone(), row);
                    let Outcome::Reaped(mut rows) = state.apply_at(tenant, op, Utc::now())? else {
                        unreachable!()
                    };
                    let row = state.jobs.values().next().unwrap();
                    if self
                        .cas(
                            &key,
                            serde_json::to_vec(row).map_err(error)?,
                            entry.revision,
                        )
                        .await?
                    {
                        outcomes.append(&mut rows);
                        if row.record.status.is_terminal() {
                            self.retire(tenant, &key).await?;
                        }
                        break;
                    }
                }
            }
            return Ok(Outcome::Reaped(outcomes));
        }
        let mut skipped_hints = std::collections::HashSet::new();
        for _ in 0..64 {
            let id = match op {
                Operation::Dequeue(queues, _) => {
                    let Some(entries) = index.tenant(tenant) else {
                        return Ok(Outcome::Lease(None));
                    };
                    let now = Utc::now();
                    let mut candidate: Option<(
                        std::cmp::Reverse<crate::JobPriority>,
                        chrono::DateTime<Utc>,
                        JobId,
                    )> = None;
                    for entry in entries.iter() {
                        let row = entry.value().1.as_ref().map_err(|e| error(e.clone()))?;
                        if !skipped_hints.contains(&row.record.job_id)
                            && queues.contains(&row.record.message.queue)
                            && row.record.message.run_at <= now
                            && row.record.status.is_eligible(now)
                        {
                            let key = (
                                std::cmp::Reverse(row.record.message.priority),
                                row.record.created_at,
                                &row.record.job_id,
                            );
                            if candidate
                                .as_ref()
                                .is_none_or(|old| key < (old.0, old.1, &old.2))
                            {
                                candidate = Some((key.0, key.1, key.2.clone()));
                            }
                        }
                    }
                    let Some((_, _, id)) = candidate else {
                        return Ok(Outcome::Lease(None));
                    };
                    id
                }
                Operation::Get(id)
                | Operation::Snapshot(id)
                | Operation::Cancel(id)
                | Operation::Complete(id, ..)
                | Operation::Fail(id, ..)
                | Operation::Heartbeat(id, ..) => id.clone(),
                _ => unreachable!(),
            };
            let (key, entry) = if matches!(op, Operation::Dequeue(..)) {
                // Watch hints can precede point-read visibility or outlive a
                // concurrent completion/purge. Neither condition is a failed
                // lookup of an acknowledged job: no owner exists until CAS.
                let key = cell(tenant, slot(&id)?);
                match self.bucket.entry(&key).await.map_err(error)? {
                    Some(entry) if entry.operation == kv::Operation::Put => {
                        let row: StoredRecord =
                            serde_json::from_slice(&entry.value).map_err(error)?;
                        if row.record.job_id != id {
                            skipped_hints.insert(id);
                            continue;
                        }
                        (key, entry)
                    }
                    _ => {
                        // Keep the hint: a remote producer's write may still
                        // become visible without another watch notification.
                        skipped_hints.insert(id);
                        continue;
                    }
                }
            } else {
                match self.read(tenant, &id).await {
                    Err(QueueError::JobNotFound(_)) if matches!(op, Operation::Cancel(_)) => {
                        return Ok(Outcome::Canceled(false))
                    }
                    other => other?,
                }
            };
            let row: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
            if key.starts_with("a.") {
                index.observe(key.clone(), entry.revision, entry.value.to_vec(), false);
            }
            let mut state = TenantState::default();
            state.jobs.insert(id.clone(), row);
            let mut outcome = state.apply_at(tenant, op, Utc::now())?;
            match &mut outcome {
                Outcome::Record(record) => {
                    record.message.payload_bytes = self.bytes(tenant, &id).await?;
                    return Ok(outcome);
                }
                Outcome::Snapshot(_) | Outcome::Canceled(false) => return Ok(outcome),
                Outcome::Lease(None) => {
                    skipped_hints.insert(id);
                    continue;
                }
                _ => {}
            }
            let row = &state.jobs[&id];
            if self
                .cas(
                    &key,
                    serde_json::to_vec(row).map_err(error)?,
                    entry.revision,
                )
                .await?
            {
                if row.record.status.is_terminal() {
                    self.retire(tenant, &key).await?;
                }
                if let Outcome::Lease(Some(job)) = &mut outcome {
                    job.record.message.payload_bytes = self.bytes(tenant, &id).await?;
                }
                return Ok(outcome);
            }
        }
        if matches!(op, Operation::Dequeue(..)) && !skipped_hints.is_empty() {
            return Ok(Outcome::Lease(None));
        }
        Err(error("JetStream job contention: retry operation"))
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        let index = self.index().await?;
        let mut tenants = std::collections::HashSet::new();
        for tenant in &index.entries {
            for entry in tenant.value().iter() {
                let row = entry.value().1.as_ref().map_err(|e| error(e.clone()))?;
                tenants.insert(row.record.tenant_id.clone());
            }
        }
        Ok(tenants.into_iter().collect())
    }
}

#[async_trait]
impl StateStore for NatsStore {
    async fn update(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        tokio::time::timeout(Duration::from_secs(30),self.update_inner(tenant,op)).await.map_err(|_|error("JetStream queue operation timed out; commit outcome may be unknown; use idempotency keys"))?
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        tokio::time::timeout(Duration::from_secs(30), NatsStore::tenants(self))
            .await
            .map_err(|_| error("JetStream tenant lookup timed out"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_protocol_revision_conflicts_are_retryable_but_other_errors_are_not() {
        for code in [10071, 10164] {
            let server: async_nats::jetstream::Error = serde_json::from_value(
                serde_json::json!({"code":400,"err_code":code,"description":"conflict"}),
            )
            .unwrap();
            let error = kv::UpdateError::with_source(kv::UpdateErrorKind::Other, server);
            assert!(revision_conflict(&error));
        }
        let server: async_nats::jetstream::Error =
            serde_json::from_value(serde_json::json!({"code":400,"err_code":10002})).unwrap();
        assert!(!revision_conflict(&server));
    }
    #[tokio::test]
    #[ignore = "requires disposable JetStream"]
    async fn stale_and_not_yet_visible_watch_hints_do_not_block_other_jobs() {
        use crate::{
            backend::nats::{NatsBackend, NatsConfig},
            JobMessage, QueueBackend, QueueCtx,
        };
        let url = std::env::var("DOGRS_NATS_URL").unwrap();
        let name = format!("hints_{}", uuid::Uuid::new_v4().simple());
        let backend = NatsBackend::new(NatsConfig {
            url: url.clone(),
            subject: name.clone(),
        })
        .await
        .unwrap();
        let ctx = QueueCtx::new("hint-race");
        let message = || {
            JobMessage::new("hint", vec![1; 65536], "bytes", "q")
                .with_run_at(Utc::now() - chrono::Duration::seconds(1))
        };
        let first = backend.enqueue(ctx.clone(), message()).await.unwrap();
        let index = backend.store.index().await.unwrap();
        // Deterministically delay watch processing; successful local CAS writes
        // still install hints, as in a separate producer racing the watch replay.
        if let Some(task) = index.task.lock().unwrap().take() {
            task.abort();
        }
        let key = cell("hint-race", slot(&first).unwrap());
        let tenant_index = index.tenant("hint-race").unwrap();
        let original = tenant_index.get(&key).unwrap().value().clone();
        let leased = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        backend
            .ack_complete(ctx.clone(), first.clone(), leased.lease_token, None)
            .await
            .unwrap();
        for missing in [false, true] {
            let mut stale = original.clone();
            let row = stale.1.as_mut().unwrap();
            if missing {
                row.record.job_id = JobId::from(format!(
                    "{}_{}",
                    slot(&first).unwrap(),
                    uuid::Uuid::new_v4()
                ));
                assert!(matches!(
                    backend
                        .get_record(ctx.clone(), row.record.job_id.clone())
                        .await,
                    Err(QueueError::JobNotFound(_))
                ));
            }
            tenant_index.insert(key.clone(), stale);
            let next = backend.enqueue(ctx.clone(), message()).await.unwrap();
            let lease = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
            assert_eq!(lease.record.job_id, next);
            backend
                .ack_complete(ctx.clone(), next, lease.lease_token, None)
                .await
                .unwrap();
            assert!(backend
                .dequeue(ctx.clone(), &["q"])
                .await
                .unwrap()
                .is_none());
        }
        // An acknowledged completed record remains available through history.
        assert!(backend
            .get_record(ctx, first)
            .await
            .unwrap()
            .status
            .is_terminal());
        let js = async_nats::jetstream::new(async_nats::connect(url).await.unwrap());
        js.delete_key_value(name).await.unwrap();
    }
}
