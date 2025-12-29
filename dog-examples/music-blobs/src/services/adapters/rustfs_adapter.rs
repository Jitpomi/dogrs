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
