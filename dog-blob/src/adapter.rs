use std::sync::Arc;
use std::collections::{HashMap, BTreeSet};
use crate::{
    BlobConfig, BlobCtx, BlobError, BlobId, BlobPut, BlobReceipt, BlobResult, BlobStore,
    ByteRange, ByteStream, DefaultKeyStrategy, OpenedBlob,
    UploadCoordinator, UploadId, UploadIntent, UploadSession, BlobKeyStrategy,
    ChunkSessionId, ChunkResult, ChunkSession
};

pub struct BlobState {
    store: Arc<dyn BlobStore>,
    keys: Arc<dyn BlobKeyStrategy>,
    uploads: Option<Arc<dyn UploadCoordinator>>,
    config: BlobConfig,
    chunk_sessions: Arc<tokio::sync::Mutex<HashMap<ChunkSessionId, ChunkSession>>>,
}
/// The main blob adapter - this is what DogService implementations embed
pub struct BlobAdapter {
   state: Arc<BlobState>,
}

impl BlobState {
    /// Create a new blob state
    pub fn new<S: BlobStore + 'static>(store: S, config: BlobConfig) -> Self {
        Self {
            store: Arc::new(store),
            keys: Arc::new(DefaultKeyStrategy),
            uploads: None,
            config,
            chunk_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Create with custom key strategy
    pub fn with_key_strategy<S: BlobStore + 'static, K: BlobKeyStrategy + 'static>(
        store: S,
        keys: K,
        config: BlobConfig,
    ) -> Self {
        Self {
            store: Arc::new(store),
            keys: Arc::new(keys),
            uploads: None,
            config,
            chunk_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Add upload coordinator for multipart/resumable uploads
    pub fn with_uploads<U: UploadCoordinator + 'static>(mut self, coordinator: U) -> Self {
        self.uploads = Some(Arc::new(coordinator));
        self
    }
}

impl BlobAdapter {
    /// Create a new blob adapter from BlobState
    pub fn new(state: Arc<BlobState>) -> Self {
        Self { state }
    }

    /// Store a blob from a stream (single-shot upload)
    pub async fn put(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
        body: ByteStream,
    ) -> BlobResult<BlobReceipt> {
        // Validate size if known
        if let Some(size) = put.size_hint {
            if size > self.state.config.max_blob_bytes {
                return Err(BlobError::invalid(format!(
                    "Blob size {} exceeds maximum {}",
                    size, self.state.config.max_blob_bytes
                )));
            }
        }

        let blob_id = BlobId::new();
        let key = self.state.keys.object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        // Store the blob
        let result = self.state.store.put(
            &key,
            put.content_type.as_deref(),
            body,
        ).await?;

        // Create receipt
        let mut receipt = BlobReceipt::new(blob_id, key, result.size_bytes)
            .with_attributes(put.attributes);

        if let Some(ct) = put.content_type {
            receipt = receipt.with_content_type(ct);
        }
        if let Some(filename) = put.filename {
            receipt = receipt.with_filename(filename);
        }
        if let Some(etag) = result.etag {
            receipt = receipt.with_etag(etag);
        }
        if let Some(checksum) = result.checksum {
            receipt = receipt.with_checksum(checksum);
        }

        // Check if store supports ranges
        if self.state.store.capabilities().supports_range {
            receipt = receipt.with_range_support();
        }

        Ok(receipt)
    }

    /// Open a blob for reading
    pub async fn open(
        &self,
        ctx: BlobCtx,
        id: BlobId,
        range: Option<ByteRange>,
    ) -> BlobResult<OpenedBlob> {
        let key = self.state.keys.object_key(&ctx.tenant_id, id.as_str(), &std::collections::BTreeMap::new());

        // Try signed URL first if available and no range requested
        if range.is_none() && self.can_sign_urls() {
            if let Ok(url) = self.sign_get_url(&key, 3600).await {
                let expires_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64 + 3600;

                let receipt = self.build_receipt_from_key(&key, &id).await?;
                return Ok(OpenedBlob::signed_url(receipt, url, expires_at));
            }
        }

        // Fall back to streaming
        let get_result = self.state.store.get(&key, range).await?;
        let receipt = self.build_receipt_from_get_result(&get_result, id, key);
        
        Ok(OpenedBlob::stream(
            receipt,
            get_result.stream,
            get_result.resolved_range.map(|r| crate::ResolvedRange {
                start: r.start,
                end: r.end,
                total_size: r.total_size,
            }),
        ))
    }

    /// Delete a blob
    pub async fn delete(&self, ctx: BlobCtx, id: BlobId) -> BlobResult<()> {
        let key = self.state.keys.object_key(&ctx.tenant_id, id.as_str(), &std::collections::BTreeMap::new());
        self.state.store.delete(&key).await
    }

    /// Begin a multipart upload
    pub async fn begin_multipart(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
    ) -> BlobResult<UploadSession> {
        let uploads = self.state.uploads.as_ref().ok_or_else(|| {
            BlobError::invalid("Upload coordinator not configured")
        })?;

        let blob_id = BlobId::new();
        let key = self.state.keys.object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        let intent = UploadIntent::new(blob_id, key)
            .with_content_type(put.content_type.unwrap_or_else(|| "application/octet-stream".to_string()))
            .with_filename(put.filename.unwrap_or_default())
            .with_attributes(put.attributes)
            .with_parts(
                self.state.config.upload_rules.part_size,
                put.size_hint.map(|s| ((s + self.state.config.upload_rules.part_size - 1) / self.state.config.upload_rules.part_size) as u32)
            );

        uploads.begin(ctx, intent).await
    }

    /// Upload a part
    pub async fn upload_part(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
        part_number: u32,
        body: ByteStream,
    ) -> BlobResult<crate::PartReceipt> {
        let uploads = self.state.uploads.as_ref().ok_or_else(|| {
            BlobError::invalid("Upload coordinator not configured")
        })?;

        uploads.accept_part(ctx, &upload_id, part_number, body).await
    }

    /// Complete a multipart upload
    pub async fn complete_multipart(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<BlobReceipt> {
        let uploads = self.state.uploads.as_ref().ok_or_else(|| {
            BlobError::invalid("Upload coordinator not configured")
        })?;

        uploads.complete(ctx, &upload_id).await
    }

    /// Abort a multipart upload
    pub async fn abort_multipart(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<()> {
        let uploads = self.state.uploads.as_ref().ok_or_else(|| {
            BlobError::invalid("Upload coordinator not configured")
        })?;

        uploads.abort(ctx, &upload_id).await
    }

    /// Get upload session
    pub async fn get_upload_session(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<UploadSession> {
        let uploads = self.state.uploads.as_ref().ok_or_else(|| {
            BlobError::invalid("Upload coordinator not configured")
        })?;

        uploads.get_session(ctx, &upload_id).await
    }

    /// Check if store supports signed URLs
    fn can_sign_urls(&self) -> bool {
        // For now, assume no signed URL support
        // This can be implemented later with proper trait bounds
        false
    }

    /// Generate signed URL for reading (if supported)
    async fn sign_get_url(&self, _key: &str, _expires_in_secs: u64) -> BlobResult<String> {
        // For now, return unsupported
        // This can be implemented later with proper trait bounds
        Err(BlobError::Unsupported)
    }

    /// Build receipt from key (for signed URLs)
    async fn build_receipt_from_key(&self, key: &str, id: &BlobId) -> BlobResult<BlobReceipt> {
        let head = self.state.store.head(key).await?;
        
        let mut receipt = BlobReceipt::new(id.clone(), key.to_string(), head.size_bytes);
        
        if let Some(ct) = head.content_type {
            receipt = receipt.with_content_type(ct);
        }
        if let Some(etag) = head.etag {
            receipt = receipt.with_etag(etag);
        }
        if self.state.store.capabilities().supports_range {
            receipt = receipt.with_range_support();
        }

        Ok(receipt)
    }

    /// Build receipt from get result
    fn build_receipt_from_get_result(
        &self,
        get_result: &crate::store::GetResult,
        id: BlobId,
        key: String,
    ) -> BlobReceipt {
        let mut receipt = BlobReceipt::new(id, key, get_result.size_bytes);
        
        if let Some(ct) = &get_result.content_type {
            receipt = receipt.with_content_type(ct.clone());
        }
        if let Some(etag) = &get_result.etag {
            receipt = receipt.with_etag(etag.clone());
        }
        if self.state.store.capabilities().supports_range {
            receipt = receipt.with_range_support();
        }

        receipt
    }

    /// Get configuration
    pub fn config(&self) -> &BlobConfig {
&self.state.config
    }

    /// Check if multipart uploads are available
    pub fn supports_multipart(&self) -> bool {
self.state.uploads.is_some()
    }

    /// Check if range requests are supported
    pub fn supports_ranges(&self) -> bool {
        self.state.store.capabilities().supports_range
    }

    /// Extract file data from multipart request, handling BlobRef and base64 formats
    pub async fn extract_file_data(request_data: &serde_json::Value) -> BlobResult<Vec<u8>> {
        if let Some(blob_ref) = request_data.get("file").and_then(|v| v.as_object()) {
            // Handle BlobRef format
            if let Some(temp_path) = blob_ref.get("temp_path").and_then(|v| v.as_str()) {
                let file_bytes = tokio::fs::read(temp_path).await
                    .map_err(|e| BlobError::invalid(format!("Failed to read temp file {}: {}", temp_path, e)))?;
                
                // Clean up temp file after reading
                let _ = tokio::fs::remove_file(temp_path).await;
                
                Ok(file_bytes)
            } else {
                Err(BlobError::invalid("BlobRef missing temp_path field"))
            }
        } else if let Some(base64_data) = request_data.get("file").and_then(|v| v.as_str()) {
            // Handle legacy base64 format for compatibility
            use base64::Engine;
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(base64_data)
                .map_err(|e| BlobError::invalid(format!("Invalid base64 content: {}", e)))?;
            Ok(decoded)
        } else {
            Err(BlobError::invalid(
                "Missing or invalid 'file' field - expected BlobRef object or base64 string"
            ))
        }
    }

    /// Upload a chunk for client-side chunked uploads (e.g., Dropzone)
    pub async fn put_chunk(
        &self,
        ctx: BlobCtx,
        session_id: ChunkSessionId,
        chunk_index: u32,
        total_chunks: u32,
        put: BlobPut,
        chunk_data: Vec<u8>,
    ) -> BlobResult<ChunkResult> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Get or create chunk session
        let mut sessions = self.state.chunk_sessions.lock().await;
        let session = sessions.entry(session_id.clone()).or_insert_with(|| {
            let blob_id = BlobId::new();
            let temp_dir = format!("/tmp/dog_blob_chunks_{}", session_id.as_str());
            
            ChunkSession {
                session_id: session_id.clone(),
                blob_id,
                tenant_id: ctx.tenant_id.clone(),
                total_chunks,
                received_chunks: BTreeSet::new(),
                content_type: put.content_type.clone(),
                filename: put.filename.clone(),
                temp_dir,
                created_at: current_time,
            }
        });

        // Store this chunk to temporary file
        let chunk_path = format!("{}/chunk_{:03}", session.temp_dir, chunk_index);
        
        // Ensure temp directory exists
        if let Some(parent) = std::path::Path::new(&chunk_path).parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| BlobError::invalid(format!("Failed to create chunk directory: {}", e)))?;
        }
        
        // Write chunk data to file
        tokio::fs::write(&chunk_path, &chunk_data).await
            .map_err(|e| BlobError::invalid(format!("Failed to write chunk: {}", e)))?;
        
        // Mark chunk as received
        session.received_chunks.insert(chunk_index);
        let chunks_received = session.received_chunks.len() as u32;
        
        // Check if we have all chunks
        if chunks_received == total_chunks {
            // All chunks received - reassemble and upload
            let mut reassembled_data = Vec::new();
            for i in 0..total_chunks {
                let chunk_path = format!("{}/chunk_{:03}", session.temp_dir, i);
                let chunk_data = tokio::fs::read(&chunk_path).await
                    .map_err(|e| BlobError::invalid(format!("Failed to read chunk {}: {}", i, e)))?;
                reassembled_data.extend_from_slice(&chunk_data);
            }
            
            // Create blob put request
            let blob_put = BlobPut::new()
                .with_content_type(session.content_type.clone().unwrap_or_else(|| "application/octet-stream".to_string()))
                .with_filename(session.filename.clone().unwrap_or_else(|| "upload.bin".to_string()))
                .with_size_hint(reassembled_data.len() as u64);
            
            // Create stream from reassembled data
            let bytes = bytes::Bytes::from(reassembled_data);
            let stream = async_stream::stream! {
                yield Ok(bytes);
            };
            let stream = Box::pin(stream);
            
            // Upload the complete file
            let receipt = self.put(ctx, blob_put, stream).await?;
            
            // Clean up chunk directory
            let temp_dir = session.temp_dir.clone();
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            
            // Remove session from tracking
            sessions.remove(&session_id);
            drop(sessions);
            
            Ok(ChunkResult::Complete { receipt })
        } else {
            // Still waiting for more chunks
            drop(sessions);
            Ok(ChunkResult::Partial {
                chunks_received,
                total_chunks,
            })
        }
    }

    /// High-level convenience method for multipart uploads (handles both chunked and single uploads)
    pub async fn put_from_multipart(
        &self,
        ctx: BlobCtx,
        request_data: &serde_json::Value,
    ) -> BlobResult<ChunkResult> {
        // Extract metadata
        let filename = request_data
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("upload.bin");

        let content_type = request_data
            .get("content_type")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream");

        // Extract Dropzone chunk metadata
        let dzuuid = request_data.get("dzuuid").and_then(|v| v.as_str());
        let dzchunkindex = request_data.get("dzchunkindex").and_then(|v| {
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });
        let dztotalchunkcount = request_data.get("dztotalchunkcount").and_then(|v| {
            v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        });

        // Extract file data
        let content_bytes = Self::extract_file_data(request_data).await?;

        // Handle chunked vs single upload
        if let (Some(uuid), Some(chunk_index), Some(total_chunks)) = (dzuuid, dzchunkindex, dztotalchunkcount) {
            // Chunked upload
            let session_id = ChunkSessionId::from_string(uuid.to_string());
            let put_request = BlobPut::new()
                .with_content_type(content_type)
                .with_filename(filename);
            
            self.put_chunk(ctx, session_id, chunk_index as u32, total_chunks as u32, put_request, content_bytes).await
        } else {
            // Single file upload
            let put_request = BlobPut::new()
                .with_content_type(content_type)
                .with_filename(filename)
                .with_size_hint(content_bytes.len() as u64);

            // Create stream from content bytes
            let bytes = bytes::Bytes::from(content_bytes);
            let stream = async_stream::stream! {
                yield Ok(bytes);
            };
            let stream = Box::pin(stream);

            // Upload and wrap result in ChunkResult::Complete
            let receipt = self.put(ctx, put_request, stream).await?;
            Ok(ChunkResult::Complete { receipt })
        }
    }
}
