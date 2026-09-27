use crate::{BlobResult, ByteRange, ByteStream, UploadId};
use async_trait::async_trait;

/// Information about a stored blob
#[derive(Debug, Clone)]
pub struct BlobInfo {
    pub key: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<i64>,
    pub metadata: BlobMetadata,
}

/// Rich metadata for blobs
#[derive(Debug, Clone, Default)]
pub struct BlobMetadata {
    // Audio metadata
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub duration: Option<u32>, // seconds
    pub bitrate: Option<u32>,

    // Visual metadata
    pub thumbnail_url: Option<String>,
    pub album_art_url: Option<String>,

    // Location metadata
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub location_name: Option<String>,

    // Technical metadata
    pub mime_type: Option<String>,
    pub encoding: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,

    // Custom attributes
    pub custom: std::collections::HashMap<String, String>,
}

/// Core blob storage operations - must be implemented by all storage backends
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Optional signed URL capability; the adapter never downcasts to a vendor.
    fn signed_urls(&self) -> Option<&dyn SignedUrlBlobStore> {
        None
    }
    /// Enable downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
    /// Consume the stream completely before returning success. Errors must not
    /// expose partial objects. Retrying a completed key must atomically replace
    /// it with the supplied bytes; the coordinator relies on that property.
    async fn put(
        &self,
        key: &str,
        content_type: Option<&str>,
        stream: ByteStream,
    ) -> BlobResult<PutResult>;

    /// Store a blob from a stream with metadata
    async fn put_with_metadata(
        &self,
        key: &str,
        content_type: Option<&str>,
        _filename: Option<&str>,
        stream: ByteStream,
    ) -> BlobResult<PutResult> {
        // Default implementation falls back to regular put
        self.put(key, content_type, stream).await
    }

    /// Get a blob as a stream, optionally with range support
    async fn get(&self, key: &str, range: Option<ByteRange>) -> BlobResult<GetResult>;

    /// Get blob metadata without content
    async fn head(&self, key: &str) -> BlobResult<ObjectHead>;

    /// Delete a blob
    async fn delete(&self, key: &str) -> BlobResult<()>;

    /// List blobs with optional prefix filter
    async fn list(&self, prefix: Option<&str>, limit: Option<usize>) -> BlobResult<Vec<BlobInfo>> {
        let _ = (prefix, limit);
        Err(crate::BlobError::Unsupported)
    }

    /// Get store capabilities
    fn capabilities(&self) -> StoreCapabilities;
}

/// Optional multipart upload support
#[async_trait]
pub trait MultipartBlobStore: BlobStore {
    /// Initialize a multipart upload
    async fn init_multipart(&self, key: &str, content_type: Option<&str>) -> BlobResult<UploadId>;

    /// Upload a part
    async fn put_part(
        &self,
        upload_id: &UploadId,
        part_number: u32,
        stream: ByteStream,
    ) -> BlobResult<PartETag>;

    /// Complete multipart upload
    async fn complete_multipart(
        &self,
        upload_id: &UploadId,
        parts: Vec<CompletedPart>,
    ) -> BlobResult<PutResult>;

    /// Abort multipart upload
    async fn abort_multipart(&self, upload_id: &UploadId) -> BlobResult<()>;
}

/// Optional signed URL support
#[async_trait]
pub trait SignedUrlBlobStore: BlobStore {
    /// Generate a signed URL for reading
    async fn sign_get(&self, key: &str, expires_in_secs: u64) -> BlobResult<String>;

    /// Generate a signed URL for writing
    async fn sign_put(
        &self,
        key: &str,
        content_type: Option<&str>,
        expires_in_secs: u64,
    ) -> BlobResult<String>;
}

/// Result of a successful put operation
#[derive(Debug, Clone)]
pub struct PutResult {
    pub etag: Option<String>,
    pub size_bytes: u64,
    pub checksum: Option<String>,
}

/// Result of a get operation
pub struct GetResult {
    pub stream: ByteStream,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub resolved_range: Option<ResolvedRange>,
}

/// Metadata about a blob
#[derive(Debug, Clone)]
pub struct ObjectHead {
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<i64>,
}

/// ETag for a multipart part
#[derive(Debug, Clone)]
pub struct PartETag {
    pub part_number: u32,
    pub etag: String,
}

/// Completed part for multipart upload
#[derive(Debug, Clone)]
pub struct CompletedPart {
    pub part_number: u32,
    pub etag: String,
}

/// Resolved range information
#[derive(Debug, Clone)]
pub struct ResolvedRange {
    pub start: u64,
    pub end: u64,
    pub total_size: u64,
}

/// Store capabilities
#[derive(Debug, Clone, Default)]
pub struct StoreCapabilities {
    pub supports_range: bool,
    pub supports_multipart: bool,
    pub supports_signed_urls: bool,
    pub max_part_size: Option<u64>,
    pub min_part_size: Option<u64>,
}

impl StoreCapabilities {
    pub fn basic() -> Self {
        Self {
            supports_range: false,
            supports_multipart: false,
            supports_signed_urls: false,
            max_part_size: None,
            min_part_size: None,
        }
    }

    pub fn with_range(mut self) -> Self {
        self.supports_range = true;
        self
    }

    pub fn with_multipart(mut self, min_size: Option<u64>, max_size: Option<u64>) -> Self {
        self.supports_multipart = true;
        self.min_part_size = min_size;
        self.max_part_size = max_size;
        self
    }

    pub fn with_signed_urls(mut self) -> Self {
        self.supports_signed_urls = true;
        self
    }
}

/// Strategy for generating blob keys
pub trait BlobKeyStrategy: Send + Sync {
    /// A stable, tenant-exclusive listing prefix. None disables adapter listing.
    fn tenant_prefix(&self, _tenant_id: &str) -> Option<String> {
        None
    }

    /// Generate a key for a blob
    fn object_key(
        &self,
        tenant_id: &str,
        blob_id: &str,
        hints: &std::collections::BTreeMap<String, String>,
    ) -> String;

    /// Generate a key for a derived asset
    fn derived_key(&self, original_key: &str, kind: &str) -> String;

    /// Generate a staging key for multipart uploads
    fn staging_key(&self, tenant_id: &str, upload_id: &str, part_number: u32) -> String;
}

/// Stable v2 keys. Hex-encoded tenant components cannot collide or traverse paths.
#[derive(Debug, Clone)]
pub struct DefaultKeyStrategy;
pub(crate) fn tenant_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl BlobKeyStrategy for DefaultKeyStrategy {
    fn tenant_prefix(&self, tenant: &str) -> Option<String> {
        Some(format!("v2/{}/", tenant_component(tenant)))
    }
    fn object_key(
        &self,
        tenant: &str,
        id: &str,
        _hints: &std::collections::BTreeMap<String, String>,
    ) -> String {
        format!("v2/{}/{}", tenant_component(tenant), id)
    }
    fn derived_key(&self, original: &str, kind: &str) -> String {
        format!("{}.{}", original, tenant_component(kind))
    }
    fn staging_key(&self, tenant: &str, upload: &str, part: u32) -> String {
        format!(
            "__uploads/v2/{}/{}/part-{part:06}",
            tenant_component(tenant),
            upload
        )
    }
}
