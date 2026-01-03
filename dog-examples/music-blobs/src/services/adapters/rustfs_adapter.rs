use crate::rustfs::RustFsState;
use anyhow::Result;
use dog_blob::BlobAdapter;
use serde_json::Value;
use std::sync::Arc;

/// RustFsAdapter wraps BlobAdapter and implements music-specific methods
pub struct RustFsAdapter {
    adapter: BlobAdapter,
}

impl RustFsAdapter {
    pub fn new(state: Arc<RustFsState>) -> Self {
        // Create BlobAdapter from the BlobState inside RustFsState
        let adapter = BlobAdapter::new(state.blob_state.clone());

        Self { adapter }
    }

    /// Decode MIME-encoded filename (RFC 2047 format)
    fn decode_mime_filename(filename: &str) -> String {
        use base64::Engine;
        
        // Handle MIME encoded filenames like =?UTF-8?B?base64data?=
        if filename.starts_with("=?") && filename.ends_with("?=") {
            // Simple MIME decoding for UTF-8 Base64 encoded filenames
            if let Some(captures) = filename.strip_prefix("=?UTF-8?B?").and_then(|s| s.strip_suffix("?=")) {
                if let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(captures) {
                    if let Ok(decoded_string) = String::from_utf8(decoded_bytes) {
                        return decoded_string;
                    }
                }
            }
            // Handle multiple encoded segments (split by space)
            let segments: Vec<&str> = filename.split_whitespace().collect();
            if segments.len() > 1 {
                let mut decoded_parts = Vec::new();
                for segment in segments {
                    if let Some(captures) = segment.strip_prefix("=?UTF-8?B?").and_then(|s| s.strip_suffix("?=")) {
                        if let Ok(decoded_bytes) = base64::engine::general_purpose::STANDARD.decode(captures) {
                            if let Ok(decoded_string) = String::from_utf8(decoded_bytes) {
                                decoded_parts.push(decoded_string);
                            }
                        }
                    }
                }
                if !decoded_parts.is_empty() {
                    return decoded_parts.join("");
                }
            }
        }
        
        // Return original filename if not MIME encoded or decoding fails
        filename.to_string()
    }

    // Handle multipart form data from Dropzone
    pub async fn upload(&self, data: Value) -> Result<Value> {
        let user_id = "default"; // Will be extracted from TenantContext in future
        let ctx = dog_blob::BlobCtx::new(user_id.to_string());

        // Use dog-blob's high-level convenience method
        let result = self.adapter
            .put_from_multipart(ctx, &data)
            .await
            .map_err(|e| anyhow::anyhow!("Upload failed: {}", e))?;

        // Convert result to application response format
        match result {
            dog_blob::ChunkResult::Partial { chunks_received, total_chunks } => {
                let chunk_index = data.get("dzchunkindex")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                    .unwrap_or(0);
                let dzuuid = data.get("dzuuid").and_then(|v| v.as_str());
                
                Ok(serde_json::json!({
                    "status": "chunk_received",
                    "chunk_index": chunk_index,
                    "chunks_received": chunks_received,
                    "total_chunks": total_chunks,
                    "dzuuid": dzuuid,
                    "is_complete": false
                }))
            }
            dog_blob::ChunkResult::Complete { receipt } => {
                let dzuuid = data.get("dzuuid").and_then(|v| v.as_str());
                let total_chunks = data.get("dztotalchunkcount")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
                
                Ok(serde_json::json!({
                    "status": "uploaded",
                    "blob_id": receipt.id.to_string(),
                    "key": receipt.key,
                    "size_bytes": receipt.size_bytes,
                    "content_type": receipt.content_type,
                    "filename": receipt.filename,
                    "created_at": receipt.created_at,
                    "chunk_info": {
                        "dzuuid": dzuuid,
                        "total_chunks": total_chunks,
                        "is_complete": dzuuid.is_some()
                    }
                }))
            }
        }
    }

    pub async fn find(&self, data: Option<Value>) -> Result<Value> {
        let user_id = "default"; // Will be extracted from TenantContext in future
        let ctx = dog_blob::BlobCtx::new(user_id.to_string());

        // Extract query parameters from data if provided
        let query = data.as_ref()
            .and_then(|d| d.get("query"))
            .and_then(|q| q.as_str());

        let limit = data.as_ref()
            .and_then(|d| d.get("limit"))
            .and_then(|l| l.as_u64())
            .unwrap_or(50) as usize;

        // Use dog-blob's list method to find uploaded files
        match self.adapter.list(ctx, query, Some(limit)).await {
            Ok(blobs) => {
                let music_files: Vec<serde_json::Value> = blobs
                    .into_iter()
                    // Since files are uploaded through music service which validates audio types,
                    // all blobs in this bucket should be audio files. No need to filter by extension
                    // as keys are UUIDs, not filenames.
                    .map(|blob| {
                        // Decode MIME-encoded filename if present
                        let decoded_filename = blob.filename.as_ref().map(|f| {
                            Self::decode_mime_filename(f)
                        });
                        
                        serde_json::json!({
                            "key": blob.key,
                            "size_bytes": blob.size_bytes,
                            "content_type": blob.content_type,
                            "filename": decoded_filename,
                            "etag": blob.etag,
                            "last_modified": blob.last_modified
                        })
                    })
                    .collect();

                Ok(serde_json::json!({
                    "status": "success",
                    "count": music_files.len(),
                    "files": music_files
                }))
            }
            Err(e) => {
                Ok(serde_json::json!({
                    "status": "error",
                    "message": format!("Failed to list files: {}", e),
                    "files": []
                }))
            }
        }
    }

    pub async fn download(&self, data: Value) -> Result<Value> {
        // Implementation using self.adapter.open()
        Ok(serde_json::json!({"status": "downloaded", "data": data}))
    }

    pub async fn stream(&self, data: Value) -> Result<Value> {
        // Implementation using self.adapter.open() with streaming
        Ok(serde_json::json!({"status": "streaming", "data": data}))
    }

    pub async fn pause(&self, data: Value) -> Result<Value> {
        // Implementation for pause functionality
        Ok(serde_json::json!({"status": "paused", "data": data}))
    }

    pub async fn resume(&self, data: Value) -> Result<Value> {
        // Implementation for resume functionality
        Ok(serde_json::json!({"status": "resumed", "data": data}))
    }

    pub async fn cancel(&self, data: Value) -> Result<Value> {
        // Implementation using self.blob_adapter.delete() or abort operations
        Ok(serde_json::json!({"status": "cancelled", "data": data}))
    }
}
