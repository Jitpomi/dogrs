//! One CAS cell per active idempotency scope; immutable payload and terminal history
//! live outside the cell. A replayable watch is only a discovery hint: every claim
//! must CAS the exact observed server revision before acquiring ownership.
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
use std::{future::Future, sync::Arc, time::Duration};

fn error(e: impl std::fmt::Display) -> QueueError {
    QueueError::Internal(e.to_string())
}
// Immutable payload reads do not grant ownership. Overlap them with the claim,
// but expose bytes only after the exact metadata revision was durably claimed.
// A speculative read failure gets one fresh read after ownership is established;
// uncertain writes are never retried here.
async fn claim_and_read<C, R, F, RF>(claim: C, read: R, retry: F) -> QueueResult<Option<Vec<u8>>>
where
    C: Future<Output = QueueResult<bool>>,
    R: Future<Output = QueueResult<Vec<u8>>>,
    F: FnOnce() -> RF,
    RF: Future<Output = QueueResult<Vec<u8>>>,
{
    tokio::pin!(claim, read);
    let bytes = tokio::select! {
        won = &mut claim => {
            if !won? { return Ok(None); }
            read.await
        }
        bytes = &mut read => {
            if !claim.await? { return Ok(None); }
            bytes
        }
    };
    match bytes {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) => retry().await.map(Some),
    }
}

// NATS 2.11 may use code 10164; async-nats 0.50 only maps 10071 to
// WrongLastRevision. Inspect structured causes rather than matching error text.
pub(super) fn revision_conflict(error: &(dyn std::error::Error + 'static)) -> bool {
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
#[derive(Default)]
struct RetiredRevisions {
    revisions: std::collections::HashMap<String, u64>,
    order: std::collections::VecDeque<(String, u64)>,
}
impl RetiredRevisions {
    fn remember(&mut self, key: String, revision: u64) {
        self.revisions.insert(key.clone(), revision);
        self.order.push_back((key, revision));
        while self.order.len() > 4096 {
            let (key, revision) = self.order.pop_front().unwrap();
            if self.revisions.get(&key) == Some(&revision) {
                self.revisions.remove(&key);
            }
        }
    }
}
pub(super) struct Index {
    claims: dashmap::DashSet<(String, JobId)>,
    retired: std::sync::Mutex<RetiredRevisions>,
    entries: dashmap::DashMap<String, Arc<TenantIndex>>,
    notifications: dashmap::DashMap<String, Arc<tokio::sync::Notify>>,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}
// Local hint only. The server CAS remains the ownership authority. Cancellation
// releases the hint; a remotely committed lease is still fenced by its revision.
struct ClaimReservation<'a> {
    index: &'a Index,
    key: (String, JobId),
}
impl Drop for ClaimReservation<'_> {
    fn drop(&mut self) {
        self.index.claims.remove(&self.key);
        self.index.notification(&self.key.0).notify_waiters();
    }
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
    fn notification(&self, tenant: &str) -> Arc<tokio::sync::Notify> {
        self.notifications.entry(hex(tenant)).or_default().clone()
    }
    fn candidate(
        &self,
        tenant: &str,
        queues: &[String],
        skipped: &std::collections::HashSet<JobId>,
    ) -> QueueResult<Option<JobId>> {
        let Some(entries) = self.tenant(tenant) else {
            return Ok(None);
        };
        let now = Utc::now();
        let mut candidate: Option<(
            std::cmp::Reverse<crate::JobPriority>,
            chrono::DateTime<Utc>,
            JobId,
        )> = None;
        for entry in entries.iter() {
            let row = entry.value().1.as_ref().map_err(|e| error(e.clone()))?;
            if !self
                .claims
                .contains(&(tenant.to_owned(), row.record.job_id.clone()))
                && !skipped.contains(&row.record.job_id)
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
        Ok(candidate.map(|(_, _, id)| id))
    }
    fn observe(&self, key: String, revision: u64, value: Vec<u8>, deleted: bool) {
        use dashmap::mapref::entry::Entry;
        // Decode once per observed revision, not on every poll by every worker.
        // Keep malformed metadata as an error so it cannot silently hide jobs.
        let parsed = || serde_json::from_slice::<StoredRecord>(&value).map_err(|e| e.to_string());
        let Some((tenant, _)) = key.strip_prefix("a.").and_then(|s| s.split_once('.')) else {
            return;
        };
        let entries = self
            .entries
            .entry(tenant.into())
            .or_default()
            .value()
            .clone();
        let changed = self
            .notifications
            .get(tenant)
            .map(|entry| entry.value().clone());
        let runnable = |row: &Result<StoredRecord, String>| {
            row.as_ref().is_ok_and(|row| {
                let now = Utc::now();
                row.record.message.run_at <= now && row.record.status.is_eligible(now)
            })
        };
        // Keep a bounded high-water mark across removal. Serialize this check
        // with insertion, otherwise an older point read can race terminal removal
        // and reintroduce a completed candidate. Eviction only loses an advisory
        // optimization; authoritative reads/CAS continue to fence ownership.
        let mut retired = self.retired.lock().unwrap();
        if retired
            .revisions
            .get(&key)
            .is_some_and(|seen| *seen >= revision)
        {
            return;
        }
        let wake = match entries.entry(key) {
            Entry::Occupied(mut entry) if entry.get().0 < revision => {
                if deleted {
                    retired.remember(entry.key().clone(), revision);
                    let _ = entry.remove();
                    false
                } else {
                    let row = parsed();
                    if row.as_ref().is_ok_and(|r| r.record.status.is_terminal()) {
                        retired.remember(entry.key().clone(), revision);
                        let _ = entry.remove();
                        false
                    } else {
                        let wake = runnable(&row);
                        let _ = entry.insert((revision, row));
                        wake
                    }
                }
            }
            Entry::Vacant(entry) if deleted => {
                retired.remember(entry.key().clone(), revision);
                false
            }
            Entry::Vacant(entry) => {
                let row = parsed();
                if row.as_ref().is_ok_and(|r| r.record.status.is_terminal()) {
                    retired.remember(entry.key().clone(), revision);
                    false
                } else {
                    let wake = runnable(&row);
                    entry.insert((revision, row));
                    wake
                }
            }
            _ => false,
        };
        drop(retired);
        // Re-observing a revision or learning that another worker claimed a job
        // must not wake idle claimers into a notification/point-read feedback loop.
        if wake {
            if let Some(changed) = changed {
                changed.notify_waiters();
            }
        }
    }
}
impl NatsStore {
    async fn index(&self) -> QueueResult<&Arc<Index>> {
        self.index
            .get_or_try_init(|| async {
                let index = Arc::new(Index {
                    claims: Default::default(),
                    retired: Default::default(),
                    entries: Default::default(),
                    notifications: Default::default(),
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
        // Migration requires old writers to be stopped before this backend is
        // used. Validate each tenant once, rather than reading the obsolete v1
        // key before every claim, completion and snapshot. Concurrent first uses
        // share validation; errors are never cached as successful checks.
        let checked = self
            .checked_tenants
            .entry(tenant.into())
            .or_default()
            .clone();
        checked
            .get_or_try_init(|| self.check_legacy(tenant))
            .await?;
        Ok(())
    }
    async fn check_legacy(&self, tenant: &str) -> QueueResult<()> {
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
        if let Some(entry) = crate::diagnostics::measure(
            crate::diagnostics::NATS_POINT_READ,
            self.bucket.entry(&key),
        )
        .await
        .map_err(error)?
        {
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
        // Bound speculative I/O independently of the producer admission queue.
        let _permit = self.payload_slots.acquire().await.map_err(error)?;
        crate::diagnostics::measure(
            crate::diagnostics::NATS_PAYLOAD_READ,
            self.bucket.get(payload(tenant, id)),
        )
        .await
        .map_err(error)?
        .map(|b| b.to_vec())
        .ok_or_else(|| error("JetStream job payload missing; storage was lost"))
    }
    async fn archive(&self, tenant: &str, row: &StoredRecord) -> QueueResult<()> {
        let key = history(tenant, &row.record.job_id);
        let value = serde_json::to_vec(row).map_err(error)?;
        // Unlike KV create(), revision zero never recreates a retention tombstone.
        // A delayed archiver must not resurrect intentionally purged history.
        match self.bucket.update(&key, value.clone().into(), 0).await {
            Ok(_) => Ok(()),
            Err(err) => match crate::diagnostics::measure(
                crate::diagnostics::NATS_POINT_READ,
                self.bucket.entry(&key),
            )
            .await
            .map_err(error)?
            {
                Some(old) if old.operation != kv::Operation::Put || old.value.as_ref() == value => {
                    Ok(())
                }
                _ => Err(error(err)),
            },
        }
    }
    async fn purge_record(&self, tenant: &str, row: &StoredRecord) -> QueueResult<()> {
        let id = &row.record.job_id;
        // Fence delayed archival before removing the current cell or payload.
        self.bucket
            .purge(history(tenant, id))
            .await
            .map_err(error)?;
        let key = cell(tenant, slot(id)?);
        if let Some(entry) = self
            .bucket
            .entry(&key)
            .await
            .map_err(error)?
            .filter(|e| e.operation == kv::Operation::Put)
        {
            let current: StoredRecord = serde_json::from_slice(&entry.value).map_err(error)?;
            if current.record.job_id == *id {
                if !current.record.status.is_terminal() {
                    return Err(error("Cannot purge an active job"));
                }
                if let Err(err) = self
                    .bucket
                    .purge_expect_revision(&key, Some(entry.revision))
                    .await
                {
                    if let Some(latest) = self
                        .bucket
                        .entry(&key)
                        .await
                        .map_err(error)?
                        .filter(|e| e.operation == kv::Operation::Put)
                    {
                        let latest: StoredRecord =
                            serde_json::from_slice(&latest.value).map_err(error)?;
                        if latest.record.job_id == *id {
                            return Err(error(err));
                        }
                    }
                }
            }
        }
        self.bucket
            .purge(payload(tenant, id))
            .await
            .map_err(error)?;
        Ok(())
    }
    async fn commit_enqueue(
        &self,
        tenant: &str,
        id: &JobId,
        key: &str,
        message: &crate::JobMessage,
        value: Vec<u8>,
        revision: u64,
    ) -> QueueResult<bool> {
        if let Some(writer) = &self.writer {
            let revision = crate::diagnostics::measure(
                crate::diagnostics::NATS_ENQUEUE_COMMIT,
                writer.enqueue(
                    payload(tenant, id),
                    message.payload_bytes.clone(),
                    key,
                    value.clone(),
                    revision,
                ),
            )
            .await?;
            if let Some(revision) = revision {
                self.index()
                    .await?
                    .observe(key.into(), revision, value, false);
                return Ok(true);
            }
            return Ok(false);
        }
        self.cas(key, value, revision).await
    }
    async fn cas(&self, key: &str, value: Vec<u8>, revision: u64) -> QueueResult<bool> {
        Ok(self.cas_revision(key, value, revision).await?.is_some())
    }
    async fn cas_revision(
        &self,
        key: &str,
        value: Vec<u8>,
        revision: u64,
    ) -> QueueResult<Option<u64>> {
        if let Some(writer) = &self.writer {
            let revision = crate::diagnostics::measure(
                crate::diagnostics::NATS_CAS,
                writer.submit(key, value.clone(), revision),
            )
            .await?;
            if let Some(revision) = revision {
                self.index()
                    .await?
                    .observe(key.into(), revision, value, false);
            }
            return Ok(revision);
        }
        match crate::diagnostics::measure(
            crate::diagnostics::NATS_CAS,
            self.bucket.update(key, value.clone().into(), revision),
        )
        .await
        {
            Ok(revision) => {
                self.index()
                    .await?
                    .observe(key.into(), revision, value, false);
                Ok(Some(revision))
            }
            Err(err)
                if err.kind() == kv::UpdateErrorKind::WrongLastRevision
                    || revision_conflict(&err) =>
            {
                Ok(None)
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
            self.retire_revision(tenant, key, &row, entry.revision)
                .await?;
        }
        Ok(())
    }
    async fn retire_revision(
        &self,
        tenant: &str,
        key: &str,
        row: &StoredRecord,
        revision: u64,
    ) -> QueueResult<()> {
        if row.record.status.is_terminal() {
            self.archive(tenant, row).await?;
            // Archive the acknowledged terminal revision before deleting it.
            // A concurrent replacement is protected by the expected revision.
            if let Err(err) = self.bucket.purge_expect_revision(key, Some(revision)).await {
                if self
                    .bucket
                    .entry(key)
                    .await
                    .map_err(error)?
                    .is_some_and(|e| e.revision == revision)
                {
                    return Err(error(err));
                }
            }
        }
        Ok(())
    }
}
impl NatsStore {
    async fn update_inner(&self, tenant: &str, op: &Operation) -> QueueResult<Outcome> {
        let stage = match op {
            Operation::Enqueue(_) => Some(crate::diagnostics::NATS_ENQUEUE_TOTAL),
            Operation::Dequeue(..) => Some(crate::diagnostics::NATS_CLAIM_TOTAL),
            Operation::Complete(..) => Some(crate::diagnostics::NATS_COMPLETE_TOTAL),
            _ => None,
        };
        let _scope = stage.map(crate::diagnostics::Scope::new);
        let _admission = if matches!(op, Operation::Enqueue(_)) {
            Some(
                crate::diagnostics::measure(
                    crate::diagnostics::NATS_ENQUEUE_SLOT,
                    self.enqueue_slots.acquire(),
                )
                .await
                .map_err(error)?,
            )
        } else {
            None
        };
        crate::diagnostics::measure(crate::diagnostics::NATS_LEGACY, self.legacy(tenant)).await?;
        let index =
            crate::diagnostics::measure(crate::diagnostics::NATS_INDEX, self.index()).await?;
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
            state.apply_at(
                tenant,
                &Operation::Enqueue(super::durable::metadata_message(message)),
                Utc::now(),
            )?;
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
            if self.writer.is_none() {
                crate::diagnostics::measure(
                    crate::diagnostics::NATS_PAYLOAD_CREATE,
                    self.bucket
                        .create(payload(tenant, &id), message.payload_bytes.clone().into()),
                )
                .await
                .map_err(error)?;
            }
            // Optimistically create a new scope with revision zero, avoiding a
            // leader read on the common first enqueue. Existing scopes (including
            // tombstones) conflict and use the dedupe/reuse path below. Only an
            // acknowledged CAS makes the persisted payload discoverable.
            if self
                .commit_enqueue(tenant, &id, &key, message, value.clone(), 0)
                .await?
            {
                return Ok(Outcome::Id(id));
            }
            for _ in 0..64 {
                let previous = crate::diagnostics::measure(
                    crate::diagnostics::NATS_POINT_READ,
                    self.bucket.entry(&key),
                )
                .await
                .map_err(error)?;
                let revision = previous.as_ref().map(|e| e.revision).unwrap_or(0);
                if let Some(entry) = previous.filter(|e| e.operation == kv::Operation::Put) {
                    let existing: StoredRecord =
                        serde_json::from_slice(&entry.value).map_err(error)?;
                    if !existing.record.status.is_terminal() {
                        index.observe(key.clone(), entry.revision, entry.value.to_vec(), false);
                        if self.writer.is_none() {
                            self.bucket
                                .purge(payload(tenant, &id))
                                .await
                                .map_err(error)?;
                        }
                        return Ok(Outcome::Id(existing.record.job_id));
                    }
                    self.archive(tenant, &existing).await?;
                }
                if self
                    .commit_enqueue(tenant, &id, &key, message, value.clone(), revision)
                    .await?
                {
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
            let active_prefix = format!("a.{}.", hex(tenant));
            let history_prefix = format!("h.{}.", hex(tenant));
            let mut keys = self.bucket.keys().await.map_err(error)?;
            let mut purged = std::collections::HashSet::new();
            while let Some(key) = keys.next().await {
                let key = key.map_err(error)?;
                if !key.starts_with(&active_prefix) && !key.starts_with(&history_prefix) {
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
                    if row.record.status.is_terminal()
                        && row.record.updated_at < *before
                        && !purged.contains(&row.record.job_id)
                    {
                        self.purge_record(tenant, &row).await?;
                        purged.insert(row.record.job_id);
                        if purged.len() >= 1000 {
                            break;
                        }
                    }
                }
            }
            return Ok(Outcome::Purged(purged.len()));
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
        if let Operation::Complete(id, ..) = op {
            let key = cell(tenant, slot(id)?);
            let cached = index.tenant(tenant).and_then(|entries| {
                entries.get(&key).and_then(|entry| {
                    let (revision, row) = entry.value();
                    row.as_ref()
                        .ok()
                        .filter(|row| *revision > 0 && row.record.job_id == *id)
                        .map(|row| (*revision, row.clone()))
                })
            });
            if let Some((revision, row)) = cached {
                let mut state = TenantState::default();
                state.jobs.insert(id.clone(), row);
                // The cache grants no authority. A valid transition must still
                // CAS the exact server revision. Stale errors (for example a
                // remotely extended lease) and CAS conflicts fall through to
                // the authoritative read below instead of escaping to callers.
                if let Ok(Outcome::Done) = state.apply_at(tenant, op, Utc::now()) {
                    if self
                        .cas(
                            &key,
                            serde_json::to_vec(&state.jobs[id]).map_err(error)?,
                            revision,
                        )
                        .await?
                    {
                        return Ok(Outcome::Done);
                    }
                }
            }
        }
        let mut skipped_hints = std::collections::HashSet::new();
        let discovery_deadline = tokio::time::Instant::now() + Duration::from_millis(50);
        for _ in 0..64 {
            let id = match op {
                Operation::Dequeue(queues, _) => {
                    // Register before inspecting the index so a remote enqueue
                    // cannot arrive between the empty check and notification wait.
                    let changed = index.notification(tenant);
                    let notified = changed.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    if let Some(id) = {
                        let _candidate =
                            crate::diagnostics::Scope::new(crate::diagnostics::NATS_CANDIDATE);
                        index.candidate(tenant, queues, &skipped_hints)?
                    } {
                        id
                    } else if tokio::time::Instant::now() < discovery_deadline {
                        let _ = crate::diagnostics::measure(
                            crate::diagnostics::NATS_DISCOVERY_WAIT,
                            tokio::time::timeout_at(discovery_deadline, notified),
                        )
                        .await;
                        continue;
                    } else {
                        return Ok(Outcome::Lease(None));
                    }
                }
                Operation::Get(id)
                | Operation::Snapshot(id)
                | Operation::Cancel(id)
                | Operation::Complete(id, ..)
                | Operation::Fail(id, ..)
                | Operation::Heartbeat(id, ..) => id.clone(),
                _ => unreachable!(),
            };
            let _reservation = if matches!(op, Operation::Dequeue(..)) {
                let key = (tenant.to_owned(), id.clone());
                if !index.claims.insert(key.clone()) {
                    skipped_hints.insert(id);
                    continue;
                }
                Some(ClaimReservation { index, key })
            } else {
                None
            };
            if matches!(op, Operation::Dequeue(..)) {
                let key = cell(tenant, slot(&id)?);
                let cached = index.tenant(tenant).and_then(|entries| {
                    entries.get(&key).and_then(|entry| {
                        let (revision, row) = entry.value();
                        row.as_ref()
                            .ok()
                            .filter(|row| *revision > 0 && row.record.job_id == id)
                            .map(|row| (*revision, row.clone()))
                    })
                });
                if let Some((revision, row)) = cached {
                    let mut state = TenantState::default();
                    state.jobs.insert(id.clone(), row);
                    if let Ok(Outcome::Lease(Some(mut job))) =
                        state.apply_at(tenant, op, Utc::now())
                    {
                        // Observed metadata is usable only while its exact server
                        // revision remains current. No lease exists until CAS
                        // succeeds; a stale or deleted hint cannot grant ownership.
                        if let Some(bytes) = claim_and_read(
                            self.cas(
                                &key,
                                serde_json::to_vec(&state.jobs[&id]).map_err(error)?,
                                revision,
                            ),
                            self.bytes(tenant, &id),
                            || self.bytes(tenant, &id),
                        )
                        .await?
                        {
                            job.record.message.payload_bytes = bytes;
                            return Ok(Outcome::Lease(Some(job)));
                        }
                        skipped_hints.insert(id);
                        continue;
                    }
                }
            }
            let (key, entry) = if matches!(op, Operation::Dequeue(..)) {
                // Watch hints can precede point-read visibility or outlive a
                // concurrent completion/purge. Neither condition is a failed
                // lookup of an acknowledged job: no owner exists until CAS.
                let key = cell(tenant, slot(&id)?);
                match crate::diagnostics::measure(
                    crate::diagnostics::NATS_POINT_READ,
                    self.bucket.entry(&key),
                )
                .await
                .map_err(error)?
                {
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
            let claim = self.cas(
                &key,
                serde_json::to_vec(row).map_err(error)?,
                entry.revision,
            );
            if let Outcome::Lease(Some(job)) = &mut outcome {
                if let Some(bytes) =
                    claim_and_read(claim, self.bytes(tenant, &id), || self.bytes(tenant, &id))
                        .await?
                {
                    job.record.message.payload_bytes = bytes;
                    return Ok(outcome);
                }
            } else if claim.await? {
                // The terminal CAS is already durable. Retain it in this cell;
                // enqueue archives it before a future idempotency-key reuse.
                return Ok(outcome);
            }
            if matches!(op, Operation::Dequeue(..)) {
                // Another worker won this revision. Do not keep selecting the
                // same advisory head while its watch update is in flight.
                // No ownership was acquired; other candidates remain eligible.
                skipped_hints.insert(id);
            }
        }
        if matches!(op, Operation::Dequeue(..)) {
            return Ok(Outcome::Lease(None));
        }
        Err(error("JetStream job contention: retry operation"))
    }
    async fn tenants(&self) -> QueueResult<Vec<String>> {
        let index =
            crate::diagnostics::measure(crate::diagnostics::NATS_INDEX, self.index()).await?;
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
    #[tokio::test]
    async fn discovery_wakes_only_for_new_runnable_revisions() {
        use futures::FutureExt;
        let index = Index {
            claims: Default::default(),
            retired: Default::default(),
            entries: Default::default(),
            notifications: Default::default(),
            task: Default::default(),
        };
        let mut row = StoredRecord {
            record: crate::JobRecord::new(
                JobId::new(),
                "test",
                crate::JobMessage::new("work", vec![], "bytes", "q"),
            ),
            token: None,
        };
        let key = cell("test", "slot");
        let changed = index.notification("test");
        let first = changed.notified();
        tokio::pin!(first);
        first.as_mut().enable();
        index.observe(key.clone(), 1, serde_json::to_vec(&row).unwrap(), false);
        assert!(first.now_or_never().is_some());
        let reservation_key = ("test".to_string(), row.record.job_id.clone());
        index.claims.insert(reservation_key.clone());
        let reservation = ClaimReservation {
            index: &index,
            key: reservation_key,
        };
        assert!(index
            .candidate("test", &["q".into()], &Default::default())
            .unwrap()
            .is_none());
        drop(reservation);
        assert_eq!(
            index
                .candidate("test", &["q".into()], &Default::default())
                .unwrap(),
            Some(row.record.job_id.clone())
        );

        let duplicate = changed.notified();
        tokio::pin!(duplicate);
        duplicate.as_mut().enable();
        index.observe(key.clone(), 1, serde_json::to_vec(&row).unwrap(), false);
        row.record.status = crate::JobStatus::Processing {
            lease_until: Utc::now() + chrono::Duration::seconds(30),
        };
        index.observe(key.clone(), 2, serde_json::to_vec(&row).unwrap(), false);
        assert!(duplicate.now_or_never().is_none());
        row.record.status = crate::JobStatus::Completed {
            completed_at: Utc::now(),
        };
        let terminal = serde_json::to_vec(&row).unwrap();
        index.observe(key.clone(), 3, terminal.clone(), false);
        assert!(index.tenant("test").unwrap().is_empty());
        row.record.status = crate::JobStatus::Enqueued;
        index.observe(key.clone(), 1, serde_json::to_vec(&row).unwrap(), false);
        assert!(
            index.tenant("test").unwrap().is_empty(),
            "delayed discovery must not resurrect a completed candidate"
        );
        row.record.job_id = JobId::new();
        index.observe(key.clone(), 4, serde_json::to_vec(&row).unwrap(), false);
        index.observe(key, 3, terminal, false);
        assert_eq!(
            index
                .candidate("test", &["q".into()], &Default::default())
                .unwrap(),
            Some(row.record.job_id)
        );
    }
    #[test]
    fn retired_revision_cache_is_bounded_even_when_one_scope_is_reused() {
        let mut cache = RetiredRevisions::default();
        for revision in 1..8192 {
            cache.remember("same".into(), revision);
        }
        assert_eq!(cache.order.len(), 4096);
        assert_eq!(cache.revisions.len(), 1);
        assert_eq!(cache.revisions["same"], 8191);
        for revision in 8192..16384 {
            cache.remember(revision.to_string(), revision);
        }
        assert_eq!(cache.order.len(), 4096);
        assert_eq!(cache.revisions.len(), 4096);
    }
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
        .unwrap()
        .with_enqueue_concurrency(1)
        .unwrap();
        // A rejected first-use migration check must not mark the tenant valid.
        let legacy_ctx = QueueCtx::new("legacy-validation");
        let legacy_key = format!("tenant_{}", hex("legacy-validation"));
        backend
            .store
            .bucket
            .put(&legacy_key, "{}".into())
            .await
            .unwrap();
        for _ in 0..2 {
            assert!(matches!(
                backend
                    .enqueue(
                        legacy_ctx.clone(),
                        JobMessage::new("legacy", vec![], "bytes", "q")
                    )
                    .await,
                Err(QueueError::InvalidConfig(_))
            ));
        }
        backend.store.bucket.purge(&legacy_key).await.unwrap();
        backend
            .enqueue(legacy_ctx, JobMessage::new("legacy", vec![], "bytes", "q"))
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
            .get_record(ctx.clone(), first)
            .await
            .unwrap()
            .status
            .is_terminal());
        // Retiring an acknowledged terminal revision must not delete a newer
        // job that reused the same idempotency slot while archival was delayed.
        let reusable = message().with_idempotency_key("retirement-race");
        let old = backend
            .enqueue(ctx.clone(), reusable.clone())
            .await
            .unwrap();
        let lease = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert_eq!(lease.record.job_id, old);
        let (key, entry) = backend.store.read("hint-race", &old).await.unwrap();
        let mut state = TenantState::default();
        state
            .jobs
            .insert(old.clone(), serde_json::from_slice(&entry.value).unwrap());
        state
            .apply_at(
                "hint-race",
                &Operation::Complete(old.clone(), lease.lease_token, None),
                Utc::now(),
            )
            .unwrap();
        let terminal = &state.jobs[&old];
        let revision = backend
            .store
            .cas_revision(&key, serde_json::to_vec(terminal).unwrap(), entry.revision)
            .await
            .unwrap()
            .unwrap();
        let replacement = backend.enqueue(ctx.clone(), reusable).await.unwrap();
        assert_ne!(old, replacement);
        backend
            .store
            .retire_revision("hint-race", &key, terminal, revision)
            .await
            .unwrap();
        let next = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert_eq!(next.record.job_id, replacement);
        assert!(backend
            .get_record(ctx.clone(), old)
            .await
            .unwrap()
            .status
            .is_terminal());
        // Saturated producer admission must not block completion of an owned job.
        let permit = backend.store.enqueue_slots.acquire().await.unwrap();
        let pending = backend.enqueue(ctx.clone(), message());
        tokio::pin!(pending);
        assert!(
            tokio::time::timeout(Duration::from_millis(5), pending.as_mut())
                .await
                .is_err()
        );
        backend
            .ack_complete(ctx.clone(), replacement, next.lease_token, None)
            .await
            .unwrap();
        drop(permit);
        let active = pending.await.unwrap();
        // Retention must cover terminal current cells and archived history,
        // preserve live jobs, and fence a delayed archiver after deletion.
        assert_eq!(
            backend
                .purge_terminal_before(ctx.clone(), Utc::now() + chrono::Duration::seconds(1))
                .await
                .unwrap(),
            5
        );
        assert_eq!(
            backend
                .get_record(ctx.clone(), active)
                .await
                .unwrap()
                .message
                .payload_bytes
                .len(),
            65536
        );
        backend.store.archive("hint-race", terminal).await.unwrap();
        assert!(matches!(
            backend
                .get_record(ctx.clone(), terminal.record.job_id.clone())
                .await,
            Err(QueueError::JobNotFound(_))
        ));
        let reused = backend
            .enqueue(
                ctx.clone(),
                message().with_idempotency_key("retirement-race"),
            )
            .await
            .unwrap();
        assert_eq!(
            backend
                .get_record(ctx, reused)
                .await
                .unwrap()
                .message
                .payload_bytes
                .len(),
            65536
        );
        let replay = NatsBackend::new(NatsConfig {
            url: url.clone(),
            subject: name.clone(),
        })
        .await
        .unwrap();
        assert_eq!(
            replay
                .store
                .index()
                .await
                .unwrap()
                .tenant("hint-race")
                .unwrap()
                .len(),
            2
        );
        let js = async_nats::jetstream::new(async_nats::connect(url).await.unwrap());
        js.delete_key_value(name).await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires disposable JetStream"]
    async fn cached_completion_rechecks_remote_cancel_and_extended_lease() {
        use crate::{
            backend::nats::{NatsBackend, NatsConfig},
            JobMessage, QueueBackend, QueueCtx,
        };
        let url = std::env::var("DOGRS_NATS_URL").unwrap();
        let name = format!("cached_complete_{}", uuid::Uuid::new_v4().simple());
        let config = NatsConfig {
            url: url.clone(),
            subject: name.clone(),
        };
        let backend = NatsBackend::new(config.clone())
            .await
            .unwrap()
            .with_lease_duration(Duration::from_secs(1));
        let remote = NatsBackend::new(config).await.unwrap();
        let ctx = QueueCtx::new("cached-completion");
        let index = backend.store.index().await.unwrap();
        if let Some(task) = index.task.lock().unwrap().take() {
            task.abort();
        }
        let id = backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("cancel", vec![7], "bytes", "q"),
            )
            .await
            .unwrap();
        let job = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert!(remote.cancel(ctx.clone(), id.clone()).await.unwrap());
        assert!(matches!(
            backend
                .ack_complete(ctx.clone(), id.clone(), job.lease_token, None)
                .await,
            Err(QueueError::JobCanceled)
        ));
        assert!(matches!(
            remote.get_record(ctx.clone(), id).await.unwrap().status,
            crate::JobStatus::Canceled { .. }
        ));
        let id = backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("extended", vec![8; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        let job = backend.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        remote
            .heartbeat_extend(
                ctx.clone(),
                id.clone(),
                job.lease_token.clone(),
                Duration::from_secs(60),
            )
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        backend
            .ack_complete(
                ctx.clone(),
                id.clone(),
                job.lease_token,
                Some("fresh lease".into()),
            )
            .await
            .unwrap();
        let row = remote.get_record(ctx.clone(), id).await.unwrap();
        assert!(matches!(row.status, crate::JobStatus::Completed { .. }));
        assert_eq!(row.result.as_deref(), Some("fresh lease"));
        assert_eq!(row.message.payload_bytes, vec![8; 65536]);
        let js = async_nats::jetstream::new(async_nats::connect(url).await.unwrap());
        js.delete_key_value(name).await.unwrap();
    }
    #[tokio::test]
    #[ignore = "requires disposable JetStream"]
    async fn cached_claim_cannot_override_remote_owner_or_create_unobserved_job() {
        use crate::{
            backend::nats::{NatsBackend, NatsConfig},
            JobMessage, QueueBackend, QueueCtx,
        };
        let url = std::env::var("DOGRS_NATS_URL").unwrap();
        let name = format!("cached_claim_{}", uuid::Uuid::new_v4().simple());
        let config = NatsConfig {
            url: url.clone(),
            subject: name.clone(),
        };
        let backend = NatsBackend::new(config.clone()).await.unwrap();
        let remote = NatsBackend::new(config).await.unwrap();
        let ctx = QueueCtx::new("cached-claim");
        let id = backend
            .enqueue(
                ctx.clone(),
                JobMessage::new("claim", vec![7; 65536], "bytes", "q"),
            )
            .await
            .unwrap();
        let index = backend.store.index().await.unwrap();
        if let Some(task) = index.task.lock().unwrap().take() {
            task.abort();
        }
        let key = cell("cached-claim", slot(&id).unwrap());
        let entries = index.tenant("cached-claim").unwrap();
        let snapshot = entries.get(&key).unwrap().value().clone();
        let owner = remote.dequeue(ctx.clone(), &["q"]).await.unwrap().unwrap();
        assert!(backend
            .dequeue(ctx.clone(), &["q"])
            .await
            .unwrap()
            .is_none());
        let fake = JobId::from(format!("{}_{}", "f".repeat(64), uuid::Uuid::new_v4()));
        let fake_key = cell("cached-claim", slot(&fake).unwrap());
        let mut unobserved = snapshot.1.unwrap();
        unobserved.record.job_id = fake;
        entries.insert(fake_key.clone(), (0, Ok(unobserved)));
        assert!(backend
            .dequeue(ctx.clone(), &["q"])
            .await
            .unwrap()
            .is_none());
        assert!(backend
            .store
            .bucket
            .entry(&fake_key)
            .await
            .unwrap()
            .is_none());
        remote
            .ack_complete(ctx.clone(), id.clone(), owner.lease_token, None)
            .await
            .unwrap();
        assert!(matches!(
            remote.get_record(ctx, id).await.unwrap().status,
            crate::JobStatus::Completed { .. }
        ));
        let js = async_nats::jetstream::new(async_nats::connect(url).await.unwrap());
        js.delete_key_value(name).await.unwrap();
    }
}

#[cfg(test)]
mod claim_read_tests {
    use super::*;
    use std::future::{pending, ready};
    use tokio::{sync::oneshot, time::timeout};

    #[tokio::test]
    async fn payload_read_starts_before_claim_but_cannot_grant_ownership() {
        let (claimed, claim) = oneshot::channel();
        let (started, read_started) = oneshot::channel();
        let task = tokio::spawn(claim_and_read(
            async { claim.await.unwrap() },
            async {
                started.send(()).unwrap();
                Ok(vec![42])
            },
            || async { panic!("successful read must not be retried") },
        ));
        timeout(Duration::from_secs(1), read_started)
            .await
            .unwrap()
            .unwrap();
        assert!(!task.is_finished());
        claimed.send(Ok(true)).unwrap();
        assert_eq!(task.await.unwrap().unwrap(), Some(vec![42]));
    }

    #[tokio::test]
    async fn lost_claim_does_not_wait_for_payload_or_retry_it() {
        let result = timeout(
            Duration::from_secs(1),
            claim_and_read(ready(Ok(false)), pending(), || async {
                panic!("lost claim must not retry payload")
            }),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn failed_speculative_read_cannot_mask_lost_or_unknown_claim() {
        for uncertain in [false, true] {
            let (claimed, claim) = oneshot::channel();
            let (read_done, done) = oneshot::channel();
            let task = tokio::spawn(claim_and_read(
                async { claim.await.unwrap() },
                async {
                    read_done.send(()).unwrap();
                    Err(error("speculative failure"))
                },
                || async { panic!("no retry without ownership") },
            ));
            done.await.unwrap();
            claimed
                .send(if uncertain {
                    Err(error("unknown commit"))
                } else {
                    Ok(false)
                })
                .unwrap();
            let result = task.await.unwrap();
            if uncertain {
                assert!(result.unwrap_err().to_string().contains("unknown commit"));
            } else {
                assert_eq!(result.unwrap(), None);
            }
        }
    }

    #[tokio::test]
    async fn readable_payload_cannot_mask_unknown_claim() {
        let result = claim_and_read(
            ready(Err(error("unknown commit"))),
            ready(Ok(vec![42])),
            || async { panic!("unknown claim must not retry") },
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("unknown commit"));
    }

    #[tokio::test]
    async fn failed_speculation_retries_only_after_winning_claim() {
        for retry_succeeds in [false, true] {
            let (claimed, claim) = oneshot::channel();
            let (read_done, done) = oneshot::channel();
            let (retry_started, mut retried) = oneshot::channel();
            let task = tokio::spawn(claim_and_read(
                async { claim.await.unwrap() },
                async {
                    read_done.send(()).unwrap();
                    Err(error("early read failure"))
                },
                move || async move {
                    retry_started.send(()).unwrap();
                    if retry_succeeds {
                        Ok(vec![42])
                    } else {
                        Err(error("final read failure"))
                    }
                },
            ));
            done.await.unwrap();
            assert!(retried.try_recv().is_err());
            claimed.send(Ok(true)).unwrap();
            let result = task.await.unwrap();
            retried.await.unwrap();
            if retry_succeeds {
                assert_eq!(result.unwrap(), Some(vec![42]));
            } else {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("final read failure"));
            }
        }
    }
}
