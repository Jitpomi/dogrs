use bytes::Bytes;
use futures_core::Stream;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::pin::Pin;
use uuid::Uuid;

/// Stream of bytes for blob content
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

/// Unique identifier for a blob
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobId(pub String);

impl BlobId {
    /// Generate a new random blob ID
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create from existing string
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for BlobId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BlobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for an upload session
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UploadId(pub String);

impl UploadId {
    /// Generate a new random upload ID
    pub fn new() -> Self {
        Self(format!("upl_{}", Uuid::new_v4().simple()))
    }

    /// Create from existing string
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for UploadId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UploadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Context for blob operations (tenant, user, request info)
#[derive(Debug, Clone)]
pub struct BlobCtx {
    pub tenant_id: String,
    pub actor_id: Option<String>,
    pub request_id: String,
}

impl BlobCtx {
    pub fn new(tenant_id: String) -> Self {
        Self {
            tenant_id,
            actor_id: None,
            request_id: Uuid::new_v4().to_string(),
        }
    }

    pub fn with_actor(mut self, actor_id: String) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = request_id;
        self
    }
}

/// Request to store a blob
#[derive(Debug, Clone)]
pub struct BlobPut {
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub size_hint: Option<u64>,
    pub attributes: serde_json::Value,
    pub key_hints: BTreeMap<String, String>,
    pub idempotency_key: Option<String>,
}

impl Default for BlobPut {
    fn default() -> Self {
        Self {
            content_type: None,
            filename: None,
            size_hint: None,
            attributes: serde_json::Value::Null,
            key_hints: BTreeMap::new(),
            idempotency_key: None,
        }
    }
}

impl BlobPut {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_content_type<S: Into<String>>(mut self, content_type: S) -> Self {
        self.content_type = Some(content_type.into());
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

    pub fn with_attribute<K: Into<String>, V: serde::Serialize>(mut self, key: K, value: V) -> Self {
        if self.attributes.is_null() {
            self.attributes = serde_json::Value::Object(serde_json::Map::new());
        }
        if let Some(obj) = self.attributes.as_object_mut() {
            obj.insert(key.into(), serde_json::to_value(value).unwrap_or(serde_json::Value::Null));
        }
        self
    }

    pub fn with_key_hint<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.key_hints.insert(key.into(), value.into());
        self
    }

    pub fn with_idempotency_key<S: Into<String>>(mut self, key: S) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }
}

/// Byte range for partial content requests
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: Option<u64>, // None means "to end of file"
}

impl ByteRange {
    pub fn new(start: u64, end: Option<u64>) -> Self {
        Self { start, end }
    }

    pub fn from_start(start: u64) -> Self {
        Self { start, end: None }
    }

    pub fn length(&self, total_size: u64) -> u64 {
        match self.end {
            Some(end) => end.saturating_sub(self.start) + 1,
            None => total_size.saturating_sub(self.start),
        }
    }

    pub fn is_valid(&self, total_size: u64) -> bool {
        if self.start >= total_size {
            return false;
        }
        if let Some(end) = self.end {
            end >= self.start && end < total_size
        } else {
            true
        }
    }
}

/// Status of an upload session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UploadStatus {
    Active,
    Completed { completed_at: i64 },
    Aborted { aborted_at: i64 },
    Failed { failed_at: i64, reason: String },
}

/// Upload session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadSession {
    pub upload_id: UploadId,
    pub blob_id: BlobId,
    pub tenant_id: String,
    pub actor_id: Option<String>,
    
    pub created_at: i64,
    pub updated_at: i64,
    
    pub total_parts: Option<u32>,
    pub status: UploadStatus,
    
    pub content_type: String,
    pub filename: Option<String>,
    pub size_hint: Option<u64>,
    pub attributes: serde_json::Value,
    
    pub progress: UploadProgress,
}

/// Progress tracking for upload sessions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadProgress {
    pub parts: BTreeMap<u32, PartReceipt>,
    pub received_bytes: u64,
}

/// Receipt for an uploaded part
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartReceipt {
    pub part_number: u32,
    pub size_bytes: u64,
    pub etag: Option<String>,
    pub checksum: Option<String>,
    pub uploaded_at: i64,
}

/// Unique identifier for a chunked upload session
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkSessionId(pub String);

impl ChunkSessionId {
    /// Create from existing string (e.g., Dropzone UUID)
    pub fn from_string(id: String) -> Self {
        Self(id)
    }

    /// Generate a new random chunk session ID
    pub fn new() -> Self {
        Self(format!("chunk_{}", Uuid::new_v4().simple()))
    }

    /// Get the inner string
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ChunkSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Result of uploading a chunk
#[derive(Debug, Clone)]
pub enum ChunkResult {
    /// Chunk received, waiting for more chunks
    Partial {
        chunks_received: u32,
        total_chunks: u32,
    },
    /// All chunks received, file assembled and uploaded
    Complete {
        receipt: crate::BlobReceipt,
    },
}

/// State tracking for a chunked upload session
#[derive(Debug, Clone)]
pub struct ChunkSession {
    pub session_id: ChunkSessionId,
    pub blob_id: BlobId,
    pub tenant_id: String,
    pub total_chunks: u32,
    pub received_chunks: std::collections::BTreeSet<u32>,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub temp_dir: String,
    pub created_at: i64,
}
