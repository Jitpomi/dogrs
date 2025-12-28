use std::sync::Arc;
use async_trait::async_trait;
use futures_util::StreamExt;

use crate::{
    BlobConfig, BlobCtx, BlobError, BlobKeyStrategy, BlobReceipt, BlobResult, BlobStore,
    ByteStream, PartReceipt, UploadCoordinator, UploadId, UploadIntent,
    UploadSession, UploadSessionStore, UploadStatus, UploadProgress, receipt::UploadInfo
};

/// Default upload coordinator that handles both native multipart and staged assembly
pub struct DefaultUploadCoordinator {
    store: Arc<dyn BlobStore>,
    sessions: Arc<dyn UploadSessionStore>,
    keys: Arc<dyn BlobKeyStrategy>,
    config: BlobConfig,
}

impl DefaultUploadCoordinator {
    pub fn new<S, SS, K>(
        store: S,
        sessions: SS,
        keys: K,
        config: BlobConfig,
    ) -> Self
    where
        S: BlobStore + 'static,
        SS: UploadSessionStore + 'static,
        K: BlobKeyStrategy + 'static,
    {
        Self {
            store: Arc::new(store),
            sessions: Arc::new(sessions),
            keys: Arc::new(keys),
            config,
        }
    }

    /// Check if we should use native multipart (always false for now - use staged)
    fn should_use_native_multipart(&self) -> bool {
        false // Simplified: always use staged assembly for now
    }

    /// Concatenate staged parts into a single stream
    fn concat_part_streams(&self, part_keys: Vec<String>) -> ByteStream {
        let store = self.store.clone();
        let stream = async_stream::stream! {
            for key in part_keys {
                match store.get(&key, None).await {
                    Ok(get_result) => {
                        let mut part_stream = get_result.stream;
                        while let Some(chunk) = StreamExt::next(&mut part_stream).await {
                            yield chunk;
                        }
                    }
                    Err(e) => {
                        yield Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Failed to read part {}: {}", key, e)
                        ));
                        return;
                    }
                }
            }
        };
        Box::pin(stream)
    }

    /// Clean up staged parts
    async fn cleanup_staged_parts(&self, tenant_id: &str, upload_id: &UploadId, part_count: u32) {
        for part_num in 1..=part_count {
            let key = self.keys.staging_key(tenant_id, upload_id.as_str(), part_num);
            let _ = self.store.delete(&key).await; // Best effort cleanup
        }
    }
}

#[async_trait]
impl UploadCoordinator for DefaultUploadCoordinator {
    async fn begin(
        &self,
        ctx: BlobCtx,
        intent: UploadIntent,
    ) -> BlobResult<UploadSession> {
        let upload_id = UploadId::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let total_parts = match &intent.chunking {
            crate::upload::Chunking::Parts { total_parts, .. } => *total_parts,
            crate::upload::Chunking::Single => None,
        };

        let session = UploadSession {
            upload_id: upload_id.clone(),
            blob_id: intent.id,
            tenant_id: ctx.tenant_id.clone(),
            actor_id: ctx.actor_id.clone(),
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

        self.sessions.create(session).await
    }

    async fn accept_part(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
        part_number: u32,
        body: ByteStream,
    ) -> BlobResult<PartReceipt> {
        // Validate part number
        if part_number == 0 || part_number > self.config.upload_rules.max_parts {
            return Err(BlobError::invalid(format!(
                "Invalid part number: {} (must be 1-{})",
                part_number, self.config.upload_rules.max_parts
            )));
        }

        let session = self.sessions.get(upload_id).await?;
        if !matches!(session.status, UploadStatus::Active) {
            return Err(BlobError::invalid("Upload session is not active"));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Use staged assembly (simplified implementation)
        let staging_key = self.keys.staging_key(&ctx.tenant_id, upload_id.as_str(), part_number);
        let result = self.store.put(&staging_key, Some("application/octet-stream"), body).await?;
        
        let receipt = PartReceipt {
            part_number,
            size_bytes: result.size_bytes,
            etag: result.etag,
            checksum: result.checksum,
            uploaded_at: now,
        };

        // Record the part
        self.sessions.record_part(upload_id, receipt.clone()).await?;

        Ok(receipt)
    }

    async fn set_total_parts(
        &self,
        _ctx: BlobCtx,
        upload_id: &UploadId,
        total_parts: u32,
    ) -> BlobResult<UploadSession> {
        let mut session = self.sessions.get(upload_id).await?;
        
        if !matches!(session.status, UploadStatus::Active) {
            return Err(BlobError::invalid("Upload session is not active"));
        }

        if total_parts == 0 || total_parts > self.config.upload_rules.max_parts {
            return Err(BlobError::invalid(format!(
                "Invalid total parts: {} (must be 1-{})",
                total_parts, self.config.upload_rules.max_parts
            )));
        }

        // Can't shrink below already uploaded max part
        if let Some(max_part) = session.progress.parts.keys().max() {
            if total_parts < *max_part {
                return Err(BlobError::invalid(format!(
                    "Cannot set total parts to {} when part {} already exists",
                    total_parts, max_part
                )));
            }
        }

        session.total_parts = Some(total_parts);
        session.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        self.sessions.update(session).await
    }

    async fn complete(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<BlobReceipt> {
        let session = self.sessions.get(upload_id).await?;
        
        if !matches!(session.status, UploadStatus::Active) {
            return Err(BlobError::invalid("Upload session is not active"));
        }

        // Determine total parts
        let total_parts = session.total_parts
            .or_else(|| session.progress.parts.keys().max().copied())
            .ok_or_else(|| BlobError::invalid("No parts uploaded"))?;

        // Validate no gaps
        for part_num in 1..=total_parts {
            if !session.progress.parts.contains_key(&part_num) {
                return Err(BlobError::invalid(format!("Missing part {}", part_num)));
            }
        }

        // Validate part sizes if strict mode
        if self.config.upload_rules.require_fixed_part_size {
            for (part_num, part) in &session.progress.parts {
                if *part_num < total_parts {
                    // Non-final part must be exactly part_size
                    if part.size_bytes != self.config.upload_rules.part_size {
                        return Err(BlobError::invalid(format!(
                            "Part {} has size {} but expected {}",
                            part_num, part.size_bytes, self.config.upload_rules.part_size
                        )));
                    }
                } else {
                    // Final part must be 1..=part_size
                    if part.size_bytes == 0 || part.size_bytes > self.config.upload_rules.part_size {
                        return Err(BlobError::invalid(format!(
                            "Final part {} has invalid size {}",
                            part_num, part.size_bytes
                        )));
                    }
                }
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let final_key = self.keys.object_key(&ctx.tenant_id, session.blob_id.as_str(), &std::collections::BTreeMap::new());

        // Staged assembly (simplified implementation)
        let part_keys: Vec<String> = (1..=total_parts)
            .map(|p| self.keys.staging_key(&ctx.tenant_id, upload_id.as_str(), p))
            .collect();

        let concatenated = self.concat_part_streams(part_keys);
        let result = self.store.put(&final_key, Some(&session.content_type), concatenated).await?;

        // Cleanup staged parts
        self.cleanup_staged_parts(&ctx.tenant_id, upload_id, total_parts).await;

        // Mark session completed
        self.sessions.mark_completed(upload_id, now).await?;

        // Build receipt
        let mut receipt = BlobReceipt::new(session.blob_id, final_key, result.size_bytes)
            .with_content_type(session.content_type)
            .with_attributes(session.attributes)
            .with_upload_info(UploadInfo::Multipart {
                upload_id: upload_id.clone(),
                part_size: self.config.upload_rules.part_size,
                parts: total_parts,
            });

        if let Some(filename) = session.filename {
            receipt = receipt.with_filename(filename);
        }
        if let Some(etag) = result.etag {
            receipt = receipt.with_etag(etag);
        }
        if let Some(checksum) = result.checksum {
            receipt = receipt.with_checksum(checksum);
        }
        if self.store.capabilities().supports_range {
            receipt = receipt.with_range_support();
        }

        Ok(receipt)
    }

    async fn abort(
        &self,
        ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<()> {
        let session = self.sessions.get(upload_id).await?;
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Cleanup staged parts (simplified implementation)
        let total_parts = session.progress.parts.keys().max().copied().unwrap_or(0);
        self.cleanup_staged_parts(&ctx.tenant_id, upload_id, total_parts).await;

        // Mark session aborted
        self.sessions.mark_aborted(upload_id, now).await?;

        Ok(())
    }

    async fn get_session(
        &self,
        _ctx: BlobCtx,
        upload_id: &UploadId,
    ) -> BlobResult<UploadSession> {
        self.sessions.get(upload_id).await
    }
}
