use crate::rustfs::RustFsState;
use crate::metadata::MimeDecoder;
use anyhow::Result;
use dog_blob::BlobAdapter;
use serde_json::Value;
use std::sync::Arc;
use futures::StreamExt;
use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::Response,
};

/// RustFsAdapter wraps BlobAdapter and implements music-specific methods
pub struct RustFsAdapter {
    adapter: BlobAdapter,
    state: Arc<RustFsState>,
}

impl RustFsAdapter {
    pub fn new(state: Arc<RustFsState>) -> Self {
        // Create BlobAdapter from the BlobState inside RustFsState
        let adapter = BlobAdapter::new(state.blob_state.clone());

        Self { adapter, state }
    }


    // Handle multipart form data from Dropzone
    pub async fn upload(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();

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
        let ctx = Self::create_default_context();

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
                    .map(|blob| Self::serialize_music_file(blob))
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

    /// Create default blob context (will be extracted from TenantContext in future)
    fn create_default_context() -> dog_blob::BlobCtx {
        dog_blob::BlobCtx::new("default".to_string())
    }


    /// Serialize a blob into music file JSON with MIME decoding
    fn serialize_music_file(blob: dog_blob::BlobInfo) -> serde_json::Value {
        serde_json::json!({
            "key": blob.key,
            "size_bytes": blob.size_bytes,
            "content_type": blob.content_type,
            "filename": MimeDecoder::decode_option(blob.filename.as_ref()),
            "etag": blob.etag,
            "last_modified": blob.last_modified,
            "metadata": {
                "title": MimeDecoder::decode_option(blob.metadata.title.as_ref()),
                "artist": MimeDecoder::decode_option(blob.metadata.artist.as_ref()),
                "album": MimeDecoder::decode_option(blob.metadata.album.as_ref()),
                "genre": blob.metadata.genre,
                "year": blob.metadata.year,
                "duration": blob.metadata.duration,
                "bitrate": blob.metadata.bitrate,
                "thumbnail_url": blob.metadata.thumbnail_url,
                "album_art_url": blob.metadata.album_art_url,
                "latitude": blob.metadata.latitude,
                "longitude": blob.metadata.longitude,
                "location_name": blob.metadata.location_name,
                "mime_type": blob.metadata.mime_type,
                "encoding": blob.metadata.encoding,
                "sample_rate": blob.metadata.sample_rate,
                "channels": blob.metadata.channels,
                "custom": blob.metadata.custom
            }
        })
    }

    pub async fn download(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();
        
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in download request"))?;
            
        println!("📥 Starting download for key: {}", key);
        
        // Extract blob ID from the key (last part after the last slash)
        let blob_id_str = key.split('/').last().unwrap_or(key);
        let blob_id = dog_blob::BlobId(blob_id_str.to_string());
        
        println!("📥 Extracted blob ID: {} from key: {}", blob_id_str, key);
        
        // Use the adapter's open method to get the blob content
        match self.adapter.open(ctx, blob_id, None).await {
            Ok(opened_blob) => {
                println!("📥 Opened blob - Size: {} bytes", opened_blob.content_length());
                
                // For now, return a simple response that indicates the blob is available
                // The frontend will need to handle this differently since we can't easily 
                // extract the raw bytes from OpenedBlob without more complex stream handling
                Ok(serde_json::json!({
                    "status": "downloaded", 
                    "key": key,
                    "size_bytes": opened_blob.content_length(),
                    "content_type": "audio/mpeg",
                    "blob_available": true,
                    "message": "Use a different approach for audio playback"
                }))
            },
            Err(e) => {
                println!("❌ Failed to open blob for download: {}", e);
                Err(anyhow::anyhow!("Failed to download audio file: {}", e))
            }
        }
    }

    pub async fn stream(&self, data: Value) -> Result<Value> {
        let ctx = Self::create_default_context();
        
        // Extract key from request data
        let key = data.get("key")
            .and_then(|k| k.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'key' field in stream request"))?;
            
        println!("🎵 Starting audio stream for key: {}", key);
        
        // Extract blob ID from the key (last part after the last slash)
        let blob_id_str = key.split('/').last().unwrap_or(key);
        let blob_id = dog_blob::BlobId(blob_id_str.to_string());
        
        println!("🎵 Extracted blob ID: {} from key: {}", blob_id_str, key);
        
        // Open the blob and read the actual audio content
        match self.adapter.open(ctx, blob_id, None).await {
            Ok(opened_blob) => {
                println!("🎵 Opened blob - Size: {} bytes", opened_blob.content_length());
                
                // Read the actual audio content using the full key
                let audio_content = self.read_blob_content(key).await?;
                
                // Return the audio content as base64 for the frontend to convert to blob
                use base64::Engine;
                let base64_content = base64::engine::general_purpose::STANDARD.encode(&audio_content);
                
                println!("🎵 Returning {} bytes of audio content as base64", audio_content.len());
                
                Ok(serde_json::json!({
                    "status": "streaming",
                    "key": key,
                    "content_type": "audio/mpeg",
                    "size_bytes": audio_content.len(),
                    "audio_data": base64_content
                }))
            },
            Err(e) => {
                println!("❌ Failed to open blob for streaming: {}", e);
                Err(anyhow::anyhow!("Failed to start audio stream: {}", e))
            }
        }
    }


    // Helper method to read blob content from the actual uploaded files
    async fn read_blob_content(&self, full_key: &str) -> Result<Vec<u8>> {
        println!("🎵 Reading actual blob content for key: {}", full_key);
        
        // Use the RustFS store directly to read the raw MP3 content
        use crate::rustfs_store::RustFSStore;
        use dog_blob::BlobStore;
        
        // Get the bucket name from environment
        let bucket = std::env::var("RUSTFS_BUCKET").unwrap_or_else(|_| "music-blobs".to_string());
        
        // Create a new RustFS store instance to read the content
        let store = RustFSStore::new(bucket).await?;
        
        // Use the full key as provided (e.g., "default/2026/01/uuid")
        println!("🎵 Fetching object with key: {}", full_key);
        
        // Read the actual content from the store
        match store.get(full_key, None).await {
            Ok(get_result) => {
                let mut content = Vec::new();
                let mut stream = get_result.stream;
                
                // Read all chunks from the stream
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            content.extend_from_slice(&chunk);
                        },
                        Err(e) => {
                            println!("❌ Error reading chunk: {}", e);
                            return Err(anyhow::anyhow!("Failed to read audio chunk: {}", e));
                        }
                    }
                }
                
                println!("🎵 Successfully read {} bytes of actual MP3 content", content.len());
                Ok(content)
            },
            Err(e) => {
                println!("❌ Failed to read from RustFS store: {}", e);
                Err(anyhow::anyhow!("Failed to read audio content from store: {}", e))
            }
        }
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
