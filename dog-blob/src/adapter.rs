use crate::bounded;
use crate::{
    BlobConfig, BlobCtx, BlobError, BlobId, BlobKeyStrategy, BlobPut, BlobReceipt, BlobResult,
    BlobStore, ByteRange, ByteStream, ChunkResult, ChunkSessionId, DefaultKeyStrategy, OpenedBlob,
    UploadCoordinator, UploadId, UploadIntent, UploadSession,
};
use futures_util::StreamExt;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

struct ChunkSlot {
    created: std::time::Instant,
    state: tokio::sync::Mutex<ChunkState>,
}
struct ChunkState {
    closed: bool,
    ctx: BlobCtx,
    total: u32,
    put: BlobPut,
    files: BTreeMap<u32, crate::ValidatedUpload>,
    parts: BTreeMap<u32, (u64, String)>,
    bytes: u64,
    completed: Option<BlobReceipt>,
}

pub struct BlobState {
    store: Arc<dyn BlobStore>,
    keys: Arc<dyn BlobKeyStrategy>,
    uploads: Option<Arc<dyn UploadCoordinator>>,
    config: BlobConfig,
    resources: crate::UploadResources,
    journal: Arc<dyn crate::UploadJournal>,
    chunk_sessions: Arc<tokio::sync::Mutex<HashMap<ChunkSessionId, Arc<ChunkSlot>>>>,
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
            journal: Arc::new(crate::MemoryUploadJournal::default()),
            resources: crate::UploadResources::from_limits(config.upload_limits.clone()),
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
            journal: Arc::new(crate::MemoryUploadJournal::default()),
            resources: crate::UploadResources::from_limits(config.upload_limits.clone()),
            config,
            chunk_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn with_journal(mut self, journal: Arc<dyn crate::UploadJournal>) -> Self {
        self.journal = journal;
        self
    }
    /// Share admission and staging budgets with other adapters/coordinators.
    pub fn with_resources(mut self, resources: crate::UploadResources) -> Self {
        self.resources = resources;
        self
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

    /// Administrative pending writes; do not expose across tenants in a public API.
    pub async fn pending_writes(&self) -> BlobResult<Vec<crate::PendingWrite>> {
        self.state.journal.list().await
    }
    pub async fn reconcile_write(&self, id: &str) -> BlobResult<Option<crate::RecoveryReport>> {
        let native = self
            .state
            .store
            .native_multipart()
            .ok_or(BlobError::Unsupported)?;
        self.state
            .resources
            .run(crate::reconcile_write(
                native,
                self.state.journal.as_ref(),
                id,
            ))
            .await
    }
    /// After saving the receipt/recovery report, remove its journal record.
    /// Uncertain writes cannot be acknowledged. Live writers return false.
    pub async fn acknowledge_write(&self, id: &str) -> BlobResult<bool> {
        let Some(mut lease) = self.state.journal.acquire(id).await? else {
            return Ok(false);
        };
        let native = self
            .state
            .store
            .native_multipart()
            .ok_or(BlobError::Unsupported)?;
        if lease.record().scope != native.recovery_scope() {
            return Err(BlobError::invalid("recovery scope mismatch"));
        }
        if native.inspect(lease.record()).await? == crate::WriteOutcome::Uncertain {
            return Err(BlobError::invalid("write outcome remains uncertain"));
        }
        lease.acknowledge().await?;
        Ok(true)
    }
    /// Store a blob from a stream (single-shot upload)
    pub async fn put(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
        body: ByteStream,
    ) -> BlobResult<BlobReceipt> {
        self.state
            .resources
            .run(self.put_inner(ctx, put, body))
            .await
    }
    async fn put_inner(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
        body: ByteStream,
    ) -> BlobResult<BlobReceipt> {
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        bounded::put_options(&put)?;
        if put
            .size_hint
            .is_some_and(|size| size > self.state.config.max_blob_bytes)
        {
            return Err(BlobError::invalid("blob exceeds byte limit"));
        }
        let staged = bounded::spool_with(
            body,
            self.state.config.max_blob_bytes,
            &self.state.resources,
        )
        .await?;
        if put.size_hint.is_some_and(|size| size != staged.size) {
            return Err(BlobError::invalid("declared and received sizes differ"));
        }
        let size = staged.size;
        let checksum = staged.checksum.clone();
        let blob_id = BlobId::new();
        let key = self
            .state
            .keys
            .object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        let (result, recovery_id) = if size >= self.state.config.multipart_threshold_bytes {
            if let Some(native) = self.state.store.native_multipart() {
                let (result, id) = crate::native::upload(
                    native,
                    self.state.journal.as_ref(),
                    &key,
                    put.content_type.as_deref(),
                    put.filename.as_deref(),
                    staged,
                    &self.state.config.upload_rules,
                )
                .await?;
                (result, Some(id))
            } else {
                (
                    self.state
                        .store
                        .put_validated(
                            &key,
                            put.content_type.as_deref(),
                            put.filename.as_deref(),
                            staged,
                        )
                        .await?,
                    None,
                )
            }
        } else {
            (
                self.state
                    .store
                    .put_validated(
                        &key,
                        put.content_type.as_deref(),
                        put.filename.as_deref(),
                        staged,
                    )
                    .await?,
                None,
            )
        };

        if result.size_bytes != size {
            let _ = self.state.store.delete(&key).await;
            return Err(BlobError::upload_failed(
                "backend returned an incorrect byte count",
            ));
        }
        // Create receipt
        let mut receipt =
            BlobReceipt::new(blob_id, key, result.size_bytes).with_attributes(put.attributes);

        receipt.recovery_id = recovery_id;
        if let Some(ct) = put.content_type {
            receipt = receipt.with_content_type(ct);
        }
        if let Some(filename) = put.filename {
            receipt = receipt.with_filename(filename);
        }
        if let Some(etag) = result.etag {
            receipt = receipt.with_etag(etag);
        }
        receipt.checksum = if self.state.config.checksum_alg.is_some() {
            Some(checksum)
        } else {
            result.checksum
        };

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
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        bounded::identifier(id.as_str())?;
        let key = self.state.keys.object_key(
            &ctx.tenant_id,
            id.as_str(),
            &std::collections::BTreeMap::new(),
        );

        // Try signed URL first if available and no range requested
        if range.is_none() && self.can_sign_urls() {
            if let Ok(url) = self.sign_get_url(&key, 3600).await {
                let expires_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
                    + 3600;

                let receipt = self.build_receipt_from_key(&key, &id).await?;
                return Ok(OpenedBlob::signed_url(receipt, url, expires_at));
            }
        }

        // Enforce explicit range policy; never silently ignore a required range.
        let range = match range {
            Some(range) if !self.supports_ranges() => {
                if self.state.config.require_range_support {
                    return Err(BlobError::Unsupported);
                }
                let _ = range;
                None
            }
            Some(range) => {
                if !range.is_valid(self.state.store.head(&key).await?.size_bytes) {
                    return Err(BlobError::invalid("invalid byte range"));
                }
                Some(range)
            }
            None => None,
        };
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
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        bounded::identifier(id.as_str())?;
        let key = self.state.keys.object_key(
            &ctx.tenant_id,
            id.as_str(),
            &std::collections::BTreeMap::new(),
        );
        self.state.store.delete(&key).await
    }

    /// Begin a multipart upload
    pub async fn begin_multipart(&self, ctx: BlobCtx, put: BlobPut) -> BlobResult<UploadSession> {
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        bounded::put_options(&put)?;
        if put
            .size_hint
            .is_some_and(|size| size > self.state.config.max_blob_bytes)
        {
            return Err(BlobError::invalid("blob exceeds byte limit"));
        }
        let total_parts = put
            .size_hint
            .map(|size| u32::try_from(size.div_ceil(self.state.config.upload_rules.part_size)))
            .transpose()
            .map_err(|_| BlobError::invalid("too many parts"))?;
        let uploads = self
            .state
            .uploads
            .as_ref()
            .ok_or_else(|| BlobError::invalid("Upload coordinator not configured"))?;

        let blob_id = BlobId::new();
        let key = self
            .state
            .keys
            .object_key(&ctx.tenant_id, blob_id.as_str(), &put.key_hints);

        let mut intent = UploadIntent::new(blob_id, key)
            .with_content_type(
                put.content_type
                    .unwrap_or_else(|| "application/octet-stream".to_string()),
            )
            .with_filename(put.filename.unwrap_or_default())
            .with_attributes(put.attributes)
            .with_parts(self.state.config.upload_rules.part_size, total_parts);

        intent.size_hint = put.size_hint;
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
        let uploads = self
            .state
            .uploads
            .as_ref()
            .ok_or_else(|| BlobError::invalid("Upload coordinator not configured"))?;

        uploads
            .accept_part(ctx, &upload_id, part_number, body)
            .await
    }

    /// Complete a multipart upload
    pub async fn complete_multipart(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<BlobReceipt> {
        let uploads = self
            .state
            .uploads
            .as_ref()
            .ok_or_else(|| BlobError::invalid("Upload coordinator not configured"))?;

        uploads.complete(ctx, &upload_id).await
    }

    /// Abort a multipart upload
    pub async fn abort_multipart(&self, ctx: BlobCtx, upload_id: UploadId) -> BlobResult<()> {
        let uploads = self
            .state
            .uploads
            .as_ref()
            .ok_or_else(|| BlobError::invalid("Upload coordinator not configured"))?;

        uploads.abort(ctx, &upload_id).await
    }

    /// Get upload session
    pub async fn get_upload_session(
        &self,
        ctx: BlobCtx,
        upload_id: UploadId,
    ) -> BlobResult<UploadSession> {
        let uploads = self
            .state
            .uploads
            .as_ref()
            .ok_or_else(|| BlobError::invalid("Upload coordinator not configured"))?;

        uploads.get_session(ctx, &upload_id).await
    }

    fn can_sign_urls(&self) -> bool {
        self.state.store.signed_urls().is_some()
    }
    async fn sign_get_url(&self, key: &str, expires_in_secs: u64) -> BlobResult<String> {
        self.state
            .store
            .signed_urls()
            .ok_or(BlobError::Unsupported)?
            .sign_get(key, expires_in_secs)
            .await
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
        let mut receipt = BlobReceipt::new(
            id,
            key,
            get_result
                .resolved_range
                .as_ref()
                .map_or(get_result.size_bytes, |r| r.total_size),
        );

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

    /// List blobs with optional prefix filter
    pub async fn list(
        &self,
        ctx: BlobCtx,
        prefix: Option<&str>,
        limit: Option<usize>,
    ) -> BlobResult<Vec<crate::BlobInfo>> {
        bounded::context(&ctx)?;
        let tenant_prefix = self
            .state
            .keys
            .tenant_prefix(&ctx.tenant_id)
            .ok_or(BlobError::Unsupported)?;
        if prefix.is_some_and(|s| s.contains("..") || s.starts_with('/') || s.contains('\\')) {
            return Err(BlobError::invalid("invalid listing prefix"));
        }
        let full_prefix = format!("{}{}", tenant_prefix, prefix.unwrap_or(""));
        self.state.store.list(Some(&full_prefix), limit).await
    }

    /// Decode a legacy base64 upload (8 MiB cap). JSON filesystem paths are never
    /// opened. Use put_file with a file handle opened by trusted server code.
    pub async fn extract_file_data(data: &serde_json::Value) -> BlobResult<Vec<u8>> {
        Self::decode_file(data, 8 * 1024 * 1024)
    }
    fn decode_file(data: &serde_json::Value, max: u64) -> BlobResult<Vec<u8>> {
        use base64::Engine;
        let encoded = data.get("file").and_then(|v| v.as_str()).ok_or_else(|| {
            BlobError::invalid("file must be base64; JSON file paths are not accepted")
        })?;
        let max = max.min(64 * 1024 * 1024);
        if encoded.len() as u64 > max.div_ceil(3) * 4 {
            return Err(BlobError::invalid("encoded file exceeds byte limit"));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| BlobError::invalid("invalid base64"))?;
        if bytes.len() as u64 > max {
            return Err(BlobError::invalid("decoded file exceeds byte limit"));
        }
        Ok(bytes)
    }
    /// The caller must establish ownership/authorization before opening this file.
    /// The library streams the handle and never deletes a caller-supplied path.
    pub async fn put_file(
        &self,
        ctx: BlobCtx,
        put: BlobPut,
        file: tokio::fs::File,
    ) -> BlobResult<BlobReceipt> {
        self.put(ctx, put, Box::pin(tokio_util::io::ReaderStream::new(file)))
            .await
    }
    /// Remove a caller-owned chunk session and its private files. This waits for
    /// any in-flight operation; it never deletes an already completed blob.
    pub async fn forget_chunk(&self, ctx: BlobCtx, id: ChunkSessionId) -> BlobResult<()> {
        bounded::context(&ctx)?;
        bounded::identifier(id.as_str())?;
        let slot = self
            .state
            .chunk_sessions
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| BlobError::invalid("chunk session unavailable"))?;
        let mut session = slot.state.lock().await;
        if session.ctx.tenant_id != ctx.tenant_id || session.ctx.actor_id != ctx.actor_id {
            return Err(BlobError::invalid("chunk session unavailable"));
        }
        session.closed = true;
        session.files.clear();
        let mut sessions = self.state.chunk_sessions.lock().await;
        if sessions
            .get(&id)
            .is_some_and(|current| Arc::ptr_eq(current, &slot))
        {
            sessions.remove(&id);
        }
        Ok(())
    }
    pub async fn forget_upload(&self, ctx: BlobCtx, id: UploadId) -> BlobResult<()> {
        self.state
            .uploads
            .as_ref()
            .ok_or(BlobError::Unsupported)?
            .forget(ctx, &id)
            .await
    }
    /// Drop expired chunk sessions and their private temporary directories.
    /// Call periodically even when no new uploads arrive.
    pub async fn cleanup_expired_chunks(&self) -> usize {
        let removed = {
            let mut sessions = self.state.chunk_sessions.lock().await;
            let ids: Vec<_> = sessions
                .iter()
                .filter(|(_, slot)| {
                    slot.created.elapsed().as_secs() >= self.state.config.session_ttl_secs
                })
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| sessions.remove(&id))
                .collect::<Vec<_>>()
        };
        let count = removed.len();
        drop(removed);
        count
    }
    pub async fn put_chunk(
        &self,
        ctx: BlobCtx,
        id: ChunkSessionId,
        index: u32,
        total: u32,
        put: BlobPut,
        bytes: Vec<u8>,
    ) -> BlobResult<ChunkResult> {
        self.state
            .resources
            .run(self.put_chunk_inner(ctx, id, index, total, put, bytes))
            .await
    }
    async fn put_chunk_inner(
        &self,
        ctx: BlobCtx,
        id: ChunkSessionId,
        index: u32,
        total: u32,
        put: BlobPut,
        bytes: Vec<u8>,
    ) -> BlobResult<ChunkResult> {
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        bounded::identifier(id.as_str())?;
        bounded::put_options(&put)?;
        let rules = &self.state.config.upload_rules;
        if put
            .size_hint
            .is_some_and(|size| size > self.state.config.max_blob_bytes)
        {
            return Err(BlobError::invalid("blob exceeds byte limit"));
        }
        if total == 0
            || total > rules.max_parts
            || index >= total
            || bytes.is_empty()
            || bytes.len() as u64 > rules.part_size
            || bytes.len() as u64 > self.state.config.max_blob_bytes
            || (rules.require_fixed_part_size
                && index < total - 1
                && bytes.len() as u64 != rules.part_size)
        {
            return Err(BlobError::invalid(
                "invalid chunk count, index or byte length",
            ));
        }
        self.cleanup_expired_chunks().await;
        let existing = self.state.chunk_sessions.lock().await.get(&id).cloned();
        let slot = if let Some(slot) = existing {
            slot
        } else {
            let new = Arc::new(ChunkSlot {
                created: std::time::Instant::now(),
                state: tokio::sync::Mutex::new(ChunkState {
                    closed: false,
                    ctx: ctx.clone(),
                    total,
                    put: put.clone(),
                    files: BTreeMap::new(),
                    parts: BTreeMap::new(),
                    bytes: 0,
                    completed: None,
                }),
            });
            let mut sessions = self.state.chunk_sessions.lock().await;
            if !sessions.contains_key(&id) && sessions.len() >= self.state.config.max_chunk_sessions
            {
                return Err(BlobError::invalid("chunk session capacity reached"));
            }
            sessions.entry(id).or_insert(new).clone()
        };
        let mut session = slot.state.lock().await;
        if session.ctx.tenant_id != ctx.tenant_id || session.ctx.actor_id != ctx.actor_id {
            return Err(BlobError::invalid("upload session unavailable"));
        }
        if session.closed || slot.created.elapsed().as_secs() >= self.state.config.session_ttl_secs
        {
            return Err(BlobError::invalid("chunk session expired"));
        }
        if session.total != total
            || session.put.content_type != put.content_type
            || session.put.filename != put.filename
            || session.put.attributes != put.attributes
            || session.put.size_hint != put.size_hint
        {
            return Err(BlobError::invalid("chunk metadata changed"));
        }
        use sha2::{Digest, Sha256};
        let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
        if let Some((size, existing)) = session.parts.get(&index) {
            if *size != bytes.len() as u64 || *existing != hash {
                return Err(BlobError::invalid("conflicting chunk retry"));
            }
        } else {
            if !rules.allow_out_of_order && index != session.parts.len() as u32 {
                return Err(BlobError::invalid("chunks must arrive in order"));
            }
            let total_bytes = session
                .bytes
                .checked_add(bytes.len() as u64)
                .filter(|n| *n <= self.state.config.max_blob_bytes)
                .ok_or_else(|| BlobError::invalid("blob exceeds byte limit"))?;
            let size = bytes.len() as u64;
            let staged = bounded::spool_with(
                Box::pin(futures::stream::once(async {
                    Ok(bytes::Bytes::from(bytes))
                })),
                rules.part_size,
                &self.state.resources,
            )
            .await?;
            session.files.insert(index, staged);
            session.parts.insert(index, (size, hash));
            session.bytes = total_bytes;
        }
        if let Some(receipt) = &session.completed {
            return Ok(ChunkResult::Complete {
                receipt: Box::new(receipt.clone()),
            });
        }
        if session.parts.len() != total as usize {
            return Ok(ChunkResult::Partial {
                chunks_received: session.parts.len() as u32,
                total_chunks: total,
            });
        }
        let paths: Vec<_> = (0..total)
            .map(|n| session.files[&n].path().to_path_buf())
            .collect();
        let body = Box::pin(async_stream::try_stream! {
            for path in paths {
                let file = tokio::fs::File::open(path).await?;
                let mut stream = tokio_util::io::ReaderStream::new(file);
                while let Some(chunk) = stream.next().await { yield chunk?; }
            }
        });
        let mut put = session.put.clone();
        if put.size_hint.is_some_and(|size| size != session.bytes) {
            return Err(BlobError::invalid("declared and received sizes differ"));
        }
        put.size_hint = Some(session.bytes);
        let receipt = self.put_inner(ctx, put, body).await?;
        session.completed = Some(receipt.clone());
        session.files.clear();
        Ok(ChunkResult::Complete {
            receipt: Box::new(receipt),
        })
    }
    pub async fn put_from_multipart(
        &self,
        ctx: BlobCtx,
        data: &serde_json::Value,
    ) -> BlobResult<ChunkResult> {
        self.state
            .resources
            .run(self.put_from_multipart_inner(ctx, data))
            .await
    }
    async fn put_from_multipart_inner(
        &self,
        ctx: BlobCtx,
        data: &serde_json::Value,
    ) -> BlobResult<ChunkResult> {
        self.state.config.validate()?;
        bounded::context(&ctx)?;
        let fields = ["dzuuid", "dzchunkindex", "dztotalchunkcount"];
        let count = fields
            .iter()
            .filter(|key| data.get(**key).is_some())
            .count();
        if count != 0 && count != 3 {
            return Err(BlobError::invalid(
                "all chunk metadata fields are required together",
            ));
        }
        let bytes = Self::decode_file(
            data,
            self.state
                .config
                .max_blob_bytes
                .min(self.state.config.max_base64_bytes)
                .min(if count == 3 {
                    self.state.config.upload_rules.part_size
                } else {
                    u64::MAX
                }),
        )?;
        let mut put = BlobPut::new();
        put.filename = data
            .get("filename")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        put.content_type = data
            .get("content_type")
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        if count == 3 {
            let id = data["dzuuid"]
                .as_str()
                .ok_or_else(|| BlobError::invalid("invalid chunk session"))?;
            let number = |key: &str| -> BlobResult<u32> {
                let value = &data[key];
                let n = value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| BlobError::invalid("invalid chunk number"))?;
                u32::try_from(n).map_err(|_| BlobError::invalid("chunk number overflow"))
            };
            self.put_chunk_inner(
                ctx,
                ChunkSessionId::from_string(id.into()),
                number("dzchunkindex")?,
                number("dztotalchunkcount")?,
                put,
                bytes,
            )
            .await
        } else {
            let stream = Box::pin(futures::stream::once(async {
                Ok(bytes::Bytes::from(bytes))
            }));
            self.put_inner(ctx, put, stream)
                .await
                .map(|receipt| ChunkResult::Complete {
                    receipt: Box::new(receipt),
                })
        }
    }
}
