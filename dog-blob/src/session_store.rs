use crate::{BlobError, BlobResult, UploadId, UploadSession, UploadSessionStore, UploadStatus};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Bounded single-process session storage. Restart durability requires a custom
/// durable store implementing the atomic compare_and_swap contract.
#[derive(Clone)]
pub struct MemoryUploadSessionStore {
    sessions: Arc<Mutex<HashMap<String, UploadSession>>>,
    capacity: usize,
}
impl Default for MemoryUploadSessionStore {
    fn default() -> Self {
        Self::new()
    }
}
impl MemoryUploadSessionStore {
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            sessions: Default::default(),
            capacity,
        }
    }
    /// Administrative cleanup inventory. Abort expired Active sessions using
    /// their trusted owner context, then forget them after successful cleanup.
    /// Completing sessions must be reconciled by retrying completion instead.
    pub fn expired(&self) -> Vec<UploadSession> {
        let now = chrono::Utc::now().timestamp();
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.expires_at <= now)
            .cloned()
            .collect()
    }
}
#[async_trait]
impl UploadSessionStore for MemoryUploadSessionStore {
    async fn create(&self, session: UploadSession) -> BlobResult<UploadSession> {
        let mut sessions = self.sessions.lock().unwrap();
        if session.revision != 0
            || session.status != UploadStatus::Active
            || !session.progress.parts.is_empty()
            || sessions.len() >= self.capacity
            || sessions.contains_key(session.upload_id.as_str())
        {
            return Err(BlobError::invalid(
                "invalid, duplicate, or over-capacity session",
            ));
        }
        sessions.insert(session.upload_id.to_string(), session.clone());
        Ok(session)
    }
    async fn get(&self, id: &UploadId) -> BlobResult<UploadSession> {
        self.sessions
            .lock()
            .unwrap()
            .get(id.as_str())
            .cloned()
            .ok_or_else(|| BlobError::upload_not_found(id.as_str()))
    }
    async fn compare_and_swap(&self, expected: u64, next: UploadSession) -> BlobResult<bool> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(current) = sessions.get(next.upload_id.as_str()) else {
            return Ok(false);
        };
        if current.revision != expected {
            return Ok(false);
        }
        let immutable = next.tenant_id == current.tenant_id
            && next.actor_id == current.actor_id
            && next.object_key == current.object_key
            && next.blob_id == current.blob_id
            && next.expires_at == current.expires_at
            && next.created_at == current.created_at
            && next.content_type == current.content_type
            && next.filename == current.filename
            && next.attributes == current.attributes
            && next.size_hint == current.size_hint;
        let status = match (&current.status, &next.status) {
            (UploadStatus::Active, UploadStatus::Active) => {
                next.completion_checksum == current.completion_checksum
            }
            (UploadStatus::Active, UploadStatus::Completing | UploadStatus::Aborted { .. }) => {
                current.progress == next.progress
                    && current.total_parts == next.total_parts
                    && next.completion_checksum == current.completion_checksum
            }
            (UploadStatus::Completing, UploadStatus::Completed { .. }) => {
                current.progress == next.progress && current.total_parts == next.total_parts
            }
            _ => false,
        };
        if !immutable || !status || Some(next.revision) != expected.checked_add(1) {
            return Err(BlobError::invalid("invalid session transition"));
        }
        sessions.insert(next.upload_id.to_string(), next);
        Ok(true)
    }
    async fn update(&self, session: UploadSession) -> BlobResult<UploadSession> {
        let expected = session.revision;
        let mut next = session;
        next.revision = expected
            .checked_add(1)
            .ok_or_else(|| BlobError::invalid("revision exhausted"))?;
        if !self.compare_and_swap(expected, next.clone()).await? {
            return Err(BlobError::invalid("session changed"));
        }
        Ok(next)
    }
    async fn delete(&self, id: &UploadId) -> BlobResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get(id.as_str()) {
            if session.status == UploadStatus::Completing
                || (session.status == UploadStatus::Active && !session.progress.parts.is_empty())
            {
                return Err(BlobError::invalid(
                    "abort or complete before deleting session metadata",
                ));
            }
        }
        sessions.remove(id.as_str());
        Ok(())
    }
}
