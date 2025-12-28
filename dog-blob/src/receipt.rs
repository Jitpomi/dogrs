use serde::{Deserialize, Serialize};
use crate::{BlobId, ByteRange, ByteStream, UploadId};

/// Receipt returned after successfully storing a blob
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobReceipt {
    pub id: BlobId,
    pub key: String,
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub etag: Option<String>,
    pub checksum: Option<String>,
    pub created_at: i64,
    pub attributes: serde_json::Value,
    pub upload: UploadInfo,
    pub accepts_ranges: bool,
}

/// Information about how the blob was uploaded
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UploadInfo {
    /// Single-shot upload
    Single {
        method: String, // "put", "signed_url", etc.
    },
    /// Multipart upload
    Multipart {
        upload_id: UploadId,
        part_size: u64,
        parts: u32,
    },
}

/// Result of opening a blob for reading
pub struct OpenedBlob {
    pub receipt: BlobReceipt,
    pub content: OpenedContent,
}

/// Content delivery method for opened blob
pub enum OpenedContent {
    /// Stream the content directly
    Stream {
        stream: ByteStream,
        resolved_range: Option<ResolvedRange>,
    },
    /// Redirect to a signed URL
    SignedUrl {
        url: String,
        expires_at: i64,
    },
}

/// Range information for partial content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedRange {
    pub start: u64,
    pub end: u64,
    pub total_size: u64,
}

impl ResolvedRange {
    pub fn from_request(range: &ByteRange, total_size: u64) -> Self {
        let end = range.end.unwrap_or(total_size - 1).min(total_size - 1);
        Self {
            start: range.start,
            end,
            total_size,
        }
    }

    pub fn content_length(&self) -> u64 {
        self.end - self.start + 1
    }

    pub fn is_full_content(&self) -> bool {
        self.start == 0 && self.end == self.total_size - 1
    }
}

impl BlobReceipt {
    /// Create a new blob receipt
    pub fn new(id: BlobId, key: String, size_bytes: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        Self {
            id,
            key,
            size_bytes,
            content_type: None,
            filename: None,
            etag: None,
            checksum: None,
            created_at: now,
            attributes: serde_json::Value::Null,
            upload: UploadInfo::Single {
                method: "put".to_string(),
            },
            accepts_ranges: false,
        }
    }

    /// Set content type
    pub fn with_content_type<S: Into<String>>(mut self, content_type: S) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    /// Set filename
    pub fn with_filename<S: Into<String>>(mut self, filename: S) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Set etag
    pub fn with_etag<S: Into<String>>(mut self, etag: S) -> Self {
        self.etag = Some(etag.into());
        self
    }

    /// Set checksum
    pub fn with_checksum<S: Into<String>>(mut self, checksum: S) -> Self {
        self.checksum = Some(checksum.into());
        self
    }

    /// Set attributes
    pub fn with_attributes(mut self, attributes: serde_json::Value) -> Self {
        self.attributes = attributes;
        self
    }

    /// Set upload info
    pub fn with_upload_info(mut self, upload: UploadInfo) -> Self {
        self.upload = upload;
        self
    }

    /// Enable range support
    pub fn with_range_support(mut self) -> Self {
        self.accepts_ranges = true;
        self
    }
}

impl OpenedBlob {
    /// Create with streaming content
    pub fn stream(receipt: BlobReceipt, stream: ByteStream, resolved_range: Option<ResolvedRange>) -> Self {
        Self {
            receipt,
            content: OpenedContent::Stream {
                stream,
                resolved_range,
            },
        }
    }

    /// Create with signed URL
    pub fn signed_url(receipt: BlobReceipt, url: String, expires_at: i64) -> Self {
        Self {
            receipt,
            content: OpenedContent::SignedUrl { url, expires_at },
        }
    }

    /// Check if this is a partial content response
    pub fn is_partial(&self) -> bool {
        match &self.content {
            OpenedContent::Stream { resolved_range, .. } => {
                resolved_range.as_ref().map_or(false, |r| !r.is_full_content())
            }
            OpenedContent::SignedUrl { .. } => false,
        }
    }

    /// Get content length
    pub fn content_length(&self) -> u64 {
        match &self.content {
            OpenedContent::Stream { resolved_range, .. } => {
                resolved_range
                    .as_ref()
                    .map_or(self.receipt.size_bytes, |r| r.content_length())
            }
            OpenedContent::SignedUrl { .. } => self.receipt.size_bytes,
        }
    }
}
