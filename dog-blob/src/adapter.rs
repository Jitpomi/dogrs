use std::sync::Arc;
use crate::{
    BlobConfig, BlobCtx, BlobError, BlobId, BlobPut, BlobReceipt, BlobResult, BlobStore,
    ByteRange, ByteStream, DefaultKeyStrategy, MultipartBlobStore, OpenedBlob, SignedUrlBlobStore,
    UploadCoordinator, UploadId, UploadIntent, UploadSession, BlobKeyStrategy
};

/// The main blob adapter - this is what DogService implementations embed
pub struct BlobAdapter {
    store: Arc<dyn BlobStore>,
    keys: Arc<dyn BlobKeyStrategy>,
    uploads: Option<Arc<dyn UploadCoordinator>>,
    config: BlobConfig,
}

impl BlobAdapter {
    /// Create a new blob adapter
    pub fn new<S: BlobStore + 'static>(store: S, config: BlobConfig) -> Self {
        Self {
            store: Arc::new(store),
            keys: Arc::new(DefaultKeyStrategy),
            uploads: None,
            config,
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
        }
    }

    /// Add upload coordinator for multipart/resumable uploads
    pub fn with_uploads<U: UploadCoordinator + 'static>(mut self, coordinator: U) -> Self {
        self.uploads = Some(Arc::new(coordinator));
        self
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
            if size > self.config.max_blob_bytes {
                return Err(BlobError::invalid(format!(
                    "Blob size {} exceeds maximum {}",
                    size, self.config.max_blob_bytes
                )));
            }
        }

        let blob_id = BlobId::new();
        let key = self.keys.object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        // Store the blob
        let result = self.store.put(
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
        if self.store.capabilities().supports_range {
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
        let key = self.keys.object_key(&ctx.tenant_id, id.as_str(), &std::collections::BTreeMap::new());

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
        let get_result = self.store.get(&key, range).await?;
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
        let key = self.keys.object_key(&ctx.tenant_id, id.as_str(), &std::collections::BTreeMap::new());
        self.store.delete(&key).await
    }

    /// Begin a multipart upload
    pub async fn begin_multipart(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
    ) -> BlobResult<UploadSession> {
        let coordinator = self.uploads.as_ref()
            .ok_or_else(|| BlobError::invalid("Multipart uploads not configured"))?;

        let blob_id = BlobId::new();
        let key = self.keys.object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        let intent = UploadIntent::new(blob_id, key)
            .with_content_type(put.content_type.unwrap_or_else(|| "application/octet-stream".to_string()))
            .with_filename(put.filename.unwrap_or_default())
            .with_attributes(put.attributes)
            .with_parts(
                self.config.upload_rules.part_size,
                put.size_hint.map(|s| ((s + self.config.upload_rules.part_size - 1) / self.config.upload_rules.part_size) as u32)
            );

        coordinator.begin(ctx, intent).await
    }

    /// Upload a part
    pub async fn upload_part(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
        part_number: u32,
        body: ByteStream,
    ) -> BlobResult<crate::PartReceipt> {
        let coordinator = self.uploads.as_ref()
            .ok_or_else(|| BlobError::invalid("Multipart uploads not configured"))?;

        coordinator.accept_part(ctx, &upload_id, part_number, body).await
    }

    /// Complete a multipart upload
    pub async fn complete_multipart(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<BlobReceipt> {
        let coordinator = self.uploads.as_ref()
            .ok_or_else(|| BlobError::invalid("Multipart uploads not configured"))?;

        coordinator.complete(ctx, &upload_id).await
    }

    /// Abort a multipart upload
    pub async fn abort_multipart(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<()> {
        let coordinator = self.uploads.as_ref()
            .ok_or_else(|| BlobError::invalid("Multipart uploads not configured"))?;

        coordinator.abort(ctx, &upload_id).await
    }

    /// Get upload session
    pub async fn get_upload_session(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<UploadSession> {
        let coordinator = self.uploads.as_ref()
            .ok_or_else(|| BlobError::invalid("Multipart uploads not configured"))?;

        coordinator.get_session(ctx, &upload_id).await
    }

    /// Check if store supports signed URLs
    fn can_sign_urls(&self) -> bool {
        // Try to downcast to SignedUrlBlobStore
        // This is a bit hacky but works for the trait object pattern
        false // For now, always stream. Can be enhanced later.
    }

    /// Generate signed URL for reading (if supported)
    async fn sign_get_url(&self, _key: &str, _expires_in_secs: u64) -> BlobResult<String> {
        Err(BlobError::Unsupported)
    }

    /// Build receipt from key (for signed URLs)
    async fn build_receipt_from_key(&self, key: &str, id: &BlobId) -> BlobResult<BlobReceipt> {
        let head = self.store.head(key).await?;
        
        let mut receipt = BlobReceipt::new(id.clone(), key.to_string(), head.size_bytes);
        
        if let Some(ct) = head.content_type {
            receipt = receipt.with_content_type(ct);
        }
        if let Some(etag) = head.etag {
            receipt = receipt.with_etag(etag);
        }
        if self.store.capabilities().supports_range {
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
        if self.store.capabilities().supports_range {
            receipt = receipt.with_range_support();
        }

        receipt
    }

    /// Get configuration
    pub fn config(&self) -> &BlobConfig {
        &self.config
    }

    /// Check if multipart uploads are available
    pub fn supports_multipart(&self) -> bool {
        self.uploads.is_some()
    }

    /// Check if range requests are supported
    pub fn supports_ranges(&self) -> bool {
        self.store.capabilities().supports_range
    }
}
