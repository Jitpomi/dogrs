use crate::{
    bounded, receipt::UploadInfo, BlobConfig, BlobCtx, BlobError, BlobKeyStrategy, BlobReceipt,
    BlobResult, BlobStore, ByteStream, PartReceipt, UploadCoordinator, UploadId, UploadIntent,
    UploadProgress, UploadSession, UploadSessionStore, UploadStatus,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use std::sync::Arc;

/// Staged multipart coordination using immutable part objects and atomic session
/// revisions. A durable UploadSessionStore enables process-restart recovery.
pub struct DefaultUploadCoordinator {
    store: Arc<dyn BlobStore>,
    sessions: Arc<dyn UploadSessionStore>,
    keys: Arc<dyn BlobKeyStrategy>,
    config: BlobConfig,
    resources: crate::UploadResources,
}
impl DefaultUploadCoordinator {
    pub fn new<
        S: BlobStore + 'static,
        SS: UploadSessionStore + 'static,
        K: BlobKeyStrategy + 'static,
    >(
        store: S,
        sessions: SS,
        keys: K,
        config: BlobConfig,
    ) -> Self {
        Self {
            store: Arc::new(store),
            sessions: Arc::new(sessions),
            keys: Arc::new(keys),
            resources: crate::UploadResources::from_limits(config.upload_limits.clone()),
            config,
        }
    }
    pub fn with_resources(mut self, resources: crate::UploadResources) -> Self {
        self.resources = resources;
        self
    }
    /// Resume frozen completion, or fence and abort an expired active session.
    /// Returns a receipt that the application must persist before forgetting.
    pub async fn recover_upload(
        &self,
        ctx: BlobCtx,
        id: &UploadId,
    ) -> BlobResult<Option<BlobReceipt>> {
        let session = self.owned(&ctx, id).await?;
        match session.status {
            UploadStatus::Completing | UploadStatus::Completed { .. } => {
                self.complete(ctx, id).await.map(Some)
            }
            UploadStatus::Active if session.expires_at <= chrono::Utc::now().timestamp() => {
                self.abort(ctx, id).await?;
                Ok(None)
            }
            UploadStatus::Aborted { .. } => {
                self.cleanup(&session).await?;
                Ok(None)
            }
            _ => Err(BlobError::invalid("session is live or requires inspection")),
        }
    }
    /// Remove one page of orphan staging objects only after the session is fenced
    /// terminal. Re-run until zero; repeat later for formerly in-flight writes.
    /// Never deletes a final object or an Active/Completing session's parts.
    pub async fn reconcile_staging(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<usize> {
        let session = self.owned(&ctx, id).await?;
        if !matches!(
            session.status,
            UploadStatus::Completed { .. } | UploadStatus::Aborted { .. }
        ) {
            return Err(BlobError::invalid("fence session before staging cleanup"));
        }
        let prefix = self
            .keys
            .staging_prefix(&ctx.tenant_id, id.as_str())
            .filter(|p| !p.is_empty())
            .ok_or(BlobError::Unsupported)?;
        let objects = self.store.list(Some(&prefix), Some(1000)).await?;
        if objects
            .iter()
            .any(|o| !o.key.starts_with(&prefix) || o.key == session.object_key)
        {
            return Err(BlobError::invalid(
                "backend returned an object outside the staging namespace",
            ));
        }
        let count = objects.len();
        for object in objects {
            self.store.delete(&object.key).await?;
        }
        Ok(count)
    }
    async fn owned(&self, ctx: &BlobCtx, id: &UploadId) -> BlobResult<UploadSession> {
        self.config.validate()?;
        bounded::context(ctx)?;
        bounded::identifier(id.as_str())?;
        let session = self.sessions.get(id).await?;
        if session.tenant_id != ctx.tenant_id || session.actor_id != ctx.actor_id {
            return Err(BlobError::upload_not_found(id.as_str()));
        }
        Ok(session)
    }
    fn active(&self, session: &UploadSession) -> BlobResult<()> {
        if session.status != UploadStatus::Active
            || session.expires_at <= chrono::Utc::now().timestamp()
        {
            return Err(BlobError::invalid("session is inactive or expired"));
        }
        Ok(())
    }
    async fn replace(&self, mut session: UploadSession) -> BlobResult<Option<UploadSession>> {
        let expected = session.revision;
        session.revision = expected
            .checked_add(1)
            .ok_or_else(|| BlobError::invalid("session revision exhausted"))?;
        session.updated_at = chrono::Utc::now().timestamp();
        if self
            .sessions
            .compare_and_swap(expected, session.clone())
            .await?
        {
            Ok(Some(session))
        } else {
            Ok(None)
        }
    }
    async fn cleanup(&self, session: &UploadSession) -> BlobResult<()> {
        // Metadata remains available when cleanup fails; abort/complete can retry.
        for part in session.progress.parts.values() {
            self.store.delete(&part.storage_key).await?;
        }
        Ok(())
    }
    fn concat(&self, session: &UploadSession) -> ByteStream {
        let parts: Vec<_> = session.progress.parts.values().cloned().collect();
        let store = self.store.clone();
        Box::pin(async_stream::try_stream! {
            use sha2::{Digest, Sha256};
            for part in parts {
                let mut stream = store.get(&part.storage_key, None).await.map_err(std::io::Error::other)?.stream;
                let mut hash = Sha256::new();
                let mut size = 0_u64;
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk?;
                    size = size.checked_add(chunk.len() as u64).ok_or_else(|| std::io::Error::other("part size overflow"))?;
                    hash.update(&chunk); yield chunk;
                }
                if size != part.size_bytes || part.checksum.as_deref() != Some(format!("sha256:{:x}", hash.finalize()).as_str()) {
                    Err(std::io::Error::other("staged part integrity mismatch"))?;
                }
            }
        })
    }
    async fn receipt(&self, session: &UploadSession) -> BlobResult<BlobReceipt> {
        let head = self.store.head(&session.object_key).await?;
        if head.size_bytes != session.progress.received_bytes {
            return Err(BlobError::upload_failed(
                "stored object size differs from committed manifest",
            ));
        }
        let mut receipt = BlobReceipt::new(
            session.blob_id.clone(),
            session.object_key.clone(),
            head.size_bytes,
        )
        .with_content_type(&session.content_type)
        .with_attributes(session.attributes.clone())
        .with_upload_info(UploadInfo::Multipart {
            upload_id: session.upload_id.clone(),
            part_size: self.config.upload_rules.part_size,
            parts: session.progress.parts.len() as u32,
        });
        receipt.filename = session.filename.clone();
        receipt.etag = head.etag;
        receipt.checksum = session.completion_checksum.clone();
        receipt.accepts_ranges = self.store.capabilities().supports_range;
        Ok(receipt)
    }
    /// Remove parts and metadata after a terminal operation. Call only after the
    /// application's receipt is durably recorded; this ends idempotent retries.
    pub async fn forget(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<()> {
        let session = self.owned(&ctx, id).await?;
        if !matches!(
            session.status,
            UploadStatus::Completed { .. } | UploadStatus::Aborted { .. }
        ) {
            return Err(BlobError::invalid(
                "only terminal sessions may be forgotten",
            ));
        }
        self.cleanup(&session).await?;
        self.sessions.delete(id).await
    }
}
impl DefaultUploadCoordinator {
    async fn accept_part_inner(
        &self,
        ctx: BlobCtx,
        id: &UploadId,
        number: u32,
        body: ByteStream,
    ) -> BlobResult<PartReceipt> {
        let mut session = self.owned(&ctx, id).await?;
        self.active(&session)?;
        if number == 0
            || number > self.config.upload_rules.max_parts
            || session.total_parts.is_some_and(|n| number > n)
        {
            return Err(BlobError::invalid("invalid part number"));
        }
        if !self.config.upload_rules.allow_out_of_order
            && !session.progress.parts.contains_key(&number)
            && number != session.progress.parts.len() as u32 + 1
        {
            return Err(BlobError::invalid("parts must arrive in order"));
        }
        let staged = bounded::spool_with(
            body,
            self.config
                .upload_rules
                .part_size
                .min(self.config.max_blob_bytes),
            &self.resources,
        )
        .await?;
        if staged.size == 0 {
            return Err(BlobError::invalid("parts must not be empty"));
        }
        if self.config.upload_rules.require_fixed_part_size
            && session.total_parts.is_some_and(|n| number < n)
            && staged.size != self.config.upload_rules.part_size
        {
            return Err(BlobError::invalid(
                "non-final part must have configured size",
            ));
        }
        let previous = session.progress.parts.get(&number).cloned();
        let total = session
            .progress
            .received_bytes
            .checked_sub(previous.as_ref().map_or(0, |p| p.size_bytes))
            .and_then(|n| n.checked_add(staged.size))
            .filter(|n| *n <= self.config.max_blob_bytes)
            .ok_or_else(|| BlobError::invalid("multipart upload exceeds byte limit"))?;
        let key = format!(
            "{}.{}",
            self.keys.staging_key(&ctx.tenant_id, id.as_str(), number),
            uuid::Uuid::new_v4()
        );
        let mut receipt = PartReceipt {
            storage_key: key.clone(),
            part_number: number,
            size_bytes: staged.size,
            checksum: Some(staged.checksum.clone()),
            etag: None,
            uploaded_at: chrono::Utc::now().timestamp(),
        };
        let result = self
            .store
            .put_validated(&key, Some("application/octet-stream"), None, staged)
            .await;
        match result {
            Ok(result) if result.size_bytes == receipt.size_bytes => receipt.etag = result.etag,
            Ok(_) => {
                let _ = self.store.delete(&key).await;
                return Err(BlobError::upload_failed("part size mismatch"));
            }
            Err(error) => {
                let _ = self.store.delete(&key).await;
                return Err(error);
            }
        }
        session.progress.parts.insert(number, receipt.clone());
        session.progress.received_bytes = total;
        // A definite CAS miss permits deleting our unique uncommitted object.
        // A store error has an unknown commit outcome: retain it for reconciliation.
        if self.replace(session).await?.is_none() {
            let _ = self.store.delete(&key).await;
            return Err(BlobError::invalid("session changed; reload and retry part"));
        }
        if let Some(previous) = previous {
            let _ = self.store.delete(&previous.storage_key).await;
        }
        Ok(receipt)
    }
}
impl DefaultUploadCoordinator {
    async fn complete_inner(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<BlobReceipt> {
        let mut session = self.owned(&ctx, id).await?;
        if matches!(session.status, UploadStatus::Completed { .. }) {
            return self.receipt(&session).await;
        }
        if session.status == UploadStatus::Active {
            self.active(&session)?;
            let total = session
                .total_parts
                .ok_or_else(|| BlobError::invalid("declare total parts before completion"))?;
            if session.progress.parts.len() != total as usize
                || !(1..=total).all(|n| session.progress.parts.contains_key(&n))
            {
                return Err(BlobError::invalid("missing or excess parts"));
            }
            if self.config.upload_rules.require_fixed_part_size
                && session.progress.parts.values().any(|p| {
                    p.part_number < total && p.size_bytes != self.config.upload_rules.part_size
                })
            {
                return Err(BlobError::invalid("non-final part size mismatch"));
            }
            if session
                .size_hint
                .is_some_and(|n| n != session.progress.received_bytes)
            {
                return Err(BlobError::invalid("declared and received sizes differ"));
            }
            session.status = UploadStatus::Completing;
            session = self
                .replace(session)
                .await?
                .ok_or_else(|| BlobError::invalid("session changed; retry completion"))?;
        }
        if session.status != UploadStatus::Completing {
            return Err(BlobError::invalid("session cannot be completed"));
        }
        // The committed manifest is immutable. Concurrent/retried writes contain
        // exactly the same bytes. Keep parts until explicit forget, even on success.
        let staged = bounded::spool_with(
            self.concat(&session),
            self.config.max_blob_bytes,
            &self.resources,
        )
        .await?;
        if staged.size != session.progress.received_bytes {
            return Err(BlobError::upload_failed("multipart data size mismatch"));
        }
        let checksum = staged.checksum.clone();
        let result = self
            .store
            .put_validated(
                &session.object_key,
                Some(&session.content_type),
                session.filename.as_deref(),
                staged,
            )
            .await?;
        if result.size_bytes != session.progress.received_bytes {
            return Err(BlobError::upload_failed("completed object size mismatch"));
        }
        session.status = UploadStatus::Completed {
            completed_at: chrono::Utc::now().timestamp(),
        };
        if self.config.checksum_alg.is_some() {
            session.completion_checksum = Some(checksum.clone());
        }
        if self.replace(session.clone()).await?.is_none() {
            session = self.owned(&ctx, id).await?;
            if !matches!(session.status, UploadStatus::Completed { .. }) {
                return Err(BlobError::invalid(
                    "completion outcome requires reconciliation",
                ));
            }
        }
        let mut receipt = self.receipt(&session).await?;
        if self.config.checksum_alg.is_some() {
            receipt.checksum = Some(checksum);
        }
        Ok(receipt)
    }
}
#[async_trait]
impl UploadCoordinator for DefaultUploadCoordinator {
    async fn forget(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<()> {
        DefaultUploadCoordinator::forget(self, ctx, id).await
    }
    async fn begin(&self, ctx: BlobCtx, intent: UploadIntent) -> BlobResult<UploadSession> {
        self.config.validate()?;
        bounded::context(&ctx)?;
        bounded::identifier(intent.id.as_str())?;
        if intent.idempotency_key.is_some() {
            return Err(BlobError::Unsupported);
        }
        if intent
            .size_hint
            .is_some_and(|size| size > self.config.max_blob_bytes)
        {
            return Err(BlobError::invalid("blob exceeds byte limit"));
        }
        let total_parts = match intent.chunking {
            crate::upload::Chunking::Parts {
                part_size,
                total_parts,
            } => {
                if part_size != self.config.upload_rules.part_size {
                    return Err(BlobError::invalid(
                        "part size must match coordinator configuration",
                    ));
                }
                total_parts
            }
            crate::upload::Chunking::Single => None,
        };
        if total_parts.is_some_and(|n| n == 0 || n > self.config.upload_rules.max_parts) {
            return Err(BlobError::invalid("invalid part count"));
        }
        let key = self
            .keys
            .object_key(&ctx.tenant_id, intent.id.as_str(), &Default::default());
        if intent.key != key {
            return Err(BlobError::invalid(
                "object key does not match tenant and blob identity",
            ));
        }
        let now = chrono::Utc::now().timestamp();
        let session = UploadSession {
            revision: 0,
            completion_checksum: None,
            expires_at: now + self.config.session_ttl_secs as i64,
            object_key: key,
            upload_id: UploadId::new(),
            blob_id: intent.id,
            tenant_id: ctx.tenant_id,
            actor_id: ctx.actor_id,
            created_at: now,
            updated_at: now,
            total_parts,
            status: UploadStatus::Active,
            content_type: intent.content_type,
            filename: intent.filename,
            size_hint: intent.size_hint,
            attributes: intent.attributes,
            progress: UploadProgress::default(),
        };
        let session = self.sessions.create(session).await?;
        // Probe CAS support before any upload I/O. Legacy stores fail closed.
        match self.replace(session.clone()).await {
            Ok(Some(session)) => Ok(session),
            Ok(None) => Err(BlobError::invalid("session changed during creation")),
            Err(error) => {
                let _ = self.sessions.delete(&session.upload_id).await;
                Err(error)
            }
        }
    }
    async fn accept_part(
        &self,
        ctx: BlobCtx,
        id: &UploadId,
        number: u32,
        body: ByteStream,
    ) -> BlobResult<PartReceipt> {
        self.resources
            .run(self.accept_part_inner(ctx, id, number, body))
            .await
    }
    async fn set_total_parts(
        &self,
        ctx: BlobCtx,
        id: &UploadId,
        total: u32,
    ) -> BlobResult<UploadSession> {
        let mut session = self.owned(&ctx, id).await?;
        self.active(&session)?;
        if total == 0
            || total > self.config.upload_rules.max_parts
            || session.progress.parts.keys().any(|n| *n > total)
        {
            return Err(BlobError::invalid("invalid total parts"));
        }
        session.total_parts = Some(total);
        self.replace(session)
            .await?
            .ok_or_else(|| BlobError::invalid("session changed; retry"))
    }
    async fn complete(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<BlobReceipt> {
        self.resources.run(self.complete_inner(ctx, id)).await
    }
    async fn abort(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<()> {
        let mut session = self.owned(&ctx, id).await?;
        if matches!(session.status, UploadStatus::Aborted { .. }) {
            return self.cleanup(&session).await;
        }
        if session.status != UploadStatus::Active {
            return Err(BlobError::invalid(
                "only active sessions can be aborted; resume an uncertain completion",
            ));
        }
        session.status = UploadStatus::Aborted {
            aborted_at: chrono::Utc::now().timestamp(),
        };
        let session = self
            .replace(session)
            .await?
            .ok_or_else(|| BlobError::invalid("session changed; retry abort"))?;
        self.cleanup(&session).await
    }
    async fn get_session(&self, ctx: BlobCtx, id: &UploadId) -> BlobResult<UploadSession> {
        self.owned(&ctx, id).await
    }
}
