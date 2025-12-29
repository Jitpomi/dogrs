use async_trait::async_trait;
use dog_blob::{
    BlobError, BlobResult, PartReceipt, UploadId, UploadSession, UploadSessionStore, UploadStatus,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Simple in-memory upload session store for the demo
#[derive(Clone)]
pub struct MemoryUploadSessionStore {
    sessions: Arc<Mutex<HashMap<String, UploadSession>>>,
}

impl MemoryUploadSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Helper method to execute operations on a session, reducing redundancy
    fn with_session_mut<F, R>(&self, upload_id: &UploadId, f: F) -> BlobResult<R>
    where
        F: FnOnce(&mut UploadSession) -> R,
    {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&upload_id.to_string())
            .ok_or_else(|| BlobError::not_found("Upload session not found"))?;
        Ok(f(session))
    }

    /// Get current timestamp
    fn current_timestamp() -> i64 {
        chrono::Utc::now().timestamp()
    }
}

#[async_trait]
impl UploadSessionStore for MemoryUploadSessionStore {
    async fn create(&self, session: UploadSession) -> BlobResult<UploadSession> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session.upload_id.to_string(), session.clone());
        Ok(session)
    }

    async fn get(&self, upload_id: &UploadId) -> BlobResult<UploadSession> {
        let sessions = self.sessions.lock().unwrap();
        sessions
            .get(&upload_id.to_string())
            .cloned()
            .ok_or_else(|| BlobError::not_found("Upload session not found"))
    }

    async fn update(&self, session: UploadSession) -> BlobResult<UploadSession> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(session.upload_id.to_string(), session.clone());
        Ok(session)
    }

    async fn delete(&self, upload_id: &UploadId) -> BlobResult<()> {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.remove(&upload_id.to_string());
        Ok(())
    }

    async fn record_part(&self, upload_id: &UploadId, receipt: PartReceipt) -> BlobResult<()> {
        self.with_session_mut(upload_id, |session| {
            session.progress.parts.insert(receipt.part_number, receipt);
            session.progress.received_bytes =
                session.progress.parts.values().map(|p| p.size_bytes).sum();
        })
    }

    async fn mark_completed(&self, upload_id: &UploadId, _timestamp: i64) -> BlobResult<()> {
        self.with_session_mut(upload_id, |session| {
            session.status = UploadStatus::Completed {
                completed_at: Self::current_timestamp(),
            };
        })
    }

    async fn mark_failed(
        &self,
        upload_id: &UploadId,
        _timestamp: i64,
        error: String,
    ) -> BlobResult<()> {
        self.with_session_mut(upload_id, |session| {
            session.status = UploadStatus::Failed {
                failed_at: Self::current_timestamp(),
                reason: error,
            };
        })
    }

    async fn mark_aborted(&self, upload_id: &UploadId, _timestamp: i64) -> BlobResult<()> {
        self.with_session_mut(upload_id, |session| {
            session.status = UploadStatus::Aborted {
                aborted_at: Self::current_timestamp(),
            };
        })
    }
}
