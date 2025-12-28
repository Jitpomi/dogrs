use async_trait::async_trait;
use crate::{
    BlobCtx, BlobId, BlobResult, PartReceipt, UploadId, UploadSession, UploadStatus, ByteStream
};

/// Coordinates multipart and resumable uploads
#[async_trait]
pub trait UploadCoordinator: Send + Sync {
    /// Begin a new upload session
    async fn begin(
        &self,
        ctx: BlobCtx,
        intent: UploadIntent,
    ) -> BlobResult<UploadSession>;

    /// Accept a part upload
    async fn accept_part(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
        part_number: u32,
        body: ByteStream,
    ) -> BlobResult<PartReceipt>;

    /// Set total parts (optional, can be done later)
    async fn set_total_parts(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
        total_parts: u32,
    ) -> BlobResult<UploadSession>;

    /// Complete the upload and return final blob receipt
    async fn complete(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<crate::BlobReceipt>;

    /// Abort the upload and cleanup
    async fn abort(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<()>;

    /// Get upload session status
    async fn get_session(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<UploadSession>;
}

/// Intent to upload a blob
#[derive(Debug, Clone)]
pub struct UploadIntent {
    pub id: BlobId,
    pub key: String,
    pub content_type: String,
    pub filename: Option<String>,
    pub size_hint: Option<u64>,
    pub attributes: serde_json::Value,
    pub chunking: Chunking,
    pub idempotency_key: Option<String>,
}

/// How the upload should be chunked
#[derive(Debug, Clone)]
pub enum Chunking {
    /// Upload in parts
    Parts {
        part_size: u64,
        total_parts: Option<u32>,
    },
    /// Single upload (no parts)
    Single,
}

/// Storage for upload session state
#[async_trait]
pub trait UploadSessionStore: Send + Sync {
    /// Create a new upload session
    async fn create(&self, session: UploadSession) -> BlobResult<UploadSession>;

    /// Get an upload session
    async fn get(&self, upload_id: &UploadId) -> BlobResult<UploadSession>;

    /// Update an upload session
    async fn update(&self, session: UploadSession) -> BlobResult<UploadSession>;

    /// Delete an upload session
    async fn delete(&self, upload_id: &UploadId) -> BlobResult<()>;

    /// Record a part upload
    async fn record_part(
        &self,
        upload_id: &UploadId,
        part: PartReceipt,
    ) -> BlobResult<()>;

    /// Mark session as completed
    async fn mark_completed(
        &self,
        upload_id: &UploadId,
        completed_at: i64,
    ) -> BlobResult<()>;

    /// Mark session as failed
    async fn mark_failed(
        &self,
        upload_id: &UploadId,
        failed_at: i64,
        reason: String,
    ) -> BlobResult<()>;

    /// Mark session as aborted
    async fn mark_aborted(
        &self,
        upload_id: &UploadId,
        aborted_at: i64,
    ) -> BlobResult<()>;
}

impl UploadIntent {
    pub fn new(id: BlobId, key: String) -> Self {
        Self {
            id,
            key,
            content_type: "application/octet-stream".to_string(),
            filename: None,
            size_hint: None,
            attributes: serde_json::Value::Null,
            chunking: Chunking::Single,
            idempotency_key: None,
        }
    }

    pub fn with_content_type<S: Into<String>>(mut self, content_type: S) -> Self {
        self.content_type = content_type.into();
        self
    }

    pub fn with_filename<S: Into<String>>(mut self, filename: S) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn with_size_hint(mut self, size: u64) -> Self {
        self.size_hint = Some(size);
        self
    }

    pub fn with_attributes(mut self, attributes: serde_json::Value) -> Self {
        self.attributes = attributes;
        self
    }

    pub fn with_parts(mut self, part_size: u64, total_parts: Option<u32>) -> Self {
        self.chunking = Chunking::Parts { part_size, total_parts };
        self
    }

    pub fn with_idempotency_key<S: Into<String>>(mut self, key: S) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}
