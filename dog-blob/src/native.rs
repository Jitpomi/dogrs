use crate::{BlobError, BlobResult, PendingWrite, PutResult, UploadJournal, ValidatedUpload};
use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOutcome {
    Committed,
    Aborted,
    Uncertain,
}
#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub record: PendingWrite,
    pub outcome: WriteOutcome,
}
#[derive(Debug, Clone)]
pub struct NativePart {
    pub number: u32,
    pub etag: String,
}
/// Optional backend capability. Implementations must atomically publish completed
/// objects, fence aborted handles, and identify committed objects by write identity.
#[async_trait]
pub trait NativeMultipartStore: Send + Sync {
    fn recovery_scope(&self) -> String;
    fn minimum_part_size(&self) -> u64;
    fn maximum_upload_size(&self) -> Option<u64> {
        None
    }
    async fn initiate(&self, record: &PendingWrite) -> BlobResult<String>;
    async fn upload_part(
        &self,
        record: &PendingWrite,
        number: u32,
        upload: &ValidatedUpload,
        offset: u64,
        length: u64,
    ) -> BlobResult<String>;
    async fn finish(&self, record: &PendingWrite, parts: Vec<NativePart>) -> BlobResult<PutResult>;
    async fn inspect(&self, record: &PendingWrite) -> BlobResult<WriteOutcome>;
    /// Abort only the exact recorded handle, never delete a completed object.
    async fn abort(&self, record: &PendingWrite) -> BlobResult<()>;
}
pub(crate) async fn upload(
    store: &dyn NativeMultipartStore,
    journal: &dyn UploadJournal,
    key: &str,
    content_type: Option<&str>,
    filename: Option<&str>,
    upload: ValidatedUpload,
    rules: &crate::UploadRules,
) -> BlobResult<(PutResult, String)> {
    if store
        .maximum_upload_size()
        .is_some_and(|limit| upload.size_bytes() > limit)
    {
        return Err(BlobError::invalid("object exceeds native backend limit"));
    }
    let part_size = rules.part_size.max(store.minimum_part_size());
    if part_size == 0 {
        return Err(BlobError::invalid("zero native part size"));
    }
    let count = upload.size_bytes().div_ceil(part_size);
    if count == 0 || count > u64::from(rules.max_parts.min(10000)) {
        return Err(BlobError::invalid("invalid native multipart part count"));
    }
    let mut record = PendingWrite {
        id: uuid::Uuid::new_v4().to_string(),
        scope: store.recovery_scope(),
        key: key.into(),
        size_bytes: upload.size_bytes(),
        checksum: upload.checksum().into(),
        content_type: content_type.map(str::to_owned),
        filename: filename.map(str::to_owned),
        native_id: None,
    };
    // Persist intention before any external mutation. Unknown initiation outcomes
    // remain explicit, rather than guessing which provider upload to delete.
    let mut lease = journal.create(record.clone()).await?;
    record.native_id = Some(store.initiate(&record).await?);
    lease.save(record.clone()).await?;
    let mut parts = Vec::new();
    for index in 0..count {
        let offset = index * part_size;
        let length = part_size.min(upload.size_bytes() - offset);
        let number = index as u32 + 1;
        let etag = store
            .upload_part(&record, number, &upload, offset, length)
            .await?;
        parts.push(NativePart { number, etag });
    }
    let result = store.finish(&record, parts).await?;
    if result.size_bytes != record.size_bytes {
        return Err(BlobError::upload_failed("native completion size mismatch"));
    }
    // Keep the record until the application saves the receipt and acknowledges it.
    Ok((result, record.id))
}
/// Administrative recovery: acquire a lease first, so live writers are skipped.
/// Completed objects are never deleted. Uncertain initiation remains in the journal.
pub async fn reconcile_write(
    store: &dyn NativeMultipartStore,
    journal: &dyn UploadJournal,
    id: &str,
) -> BlobResult<Option<RecoveryReport>> {
    let Some(lease) = journal.acquire(id).await? else {
        return Ok(None);
    };
    let record = lease.record().clone();
    if record.scope != store.recovery_scope() {
        return Err(BlobError::invalid("recovery backend scope mismatch"));
    }
    let mut outcome = store.inspect(&record).await?;
    if outcome != WriteOutcome::Committed && record.native_id.is_some() {
        store.abort(&record).await?;
        outcome = store.inspect(&record).await?;
    }
    Ok(Some(RecoveryReport { record, outcome }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryUploadJournal, UploadResources};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Backend {
        entered: tokio::sync::Notify,
        committed: AtomicBool,
        aborted: AtomicBool,
        stall: bool,
    }
    #[async_trait]
    impl NativeMultipartStore for Backend {
        fn recovery_scope(&self) -> String {
            "fixture".into()
        }
        fn minimum_part_size(&self) -> u64 {
            1
        }
        async fn initiate(&self, _: &PendingWrite) -> BlobResult<String> {
            Ok("native-handle".into())
        }
        async fn upload_part(
            &self,
            _: &PendingWrite,
            _: u32,
            _: &ValidatedUpload,
            _: u64,
            _: u64,
        ) -> BlobResult<String> {
            self.entered.notify_one();
            if self.stall {
                futures::future::pending::<()>().await;
            }
            Ok("part-etag".into())
        }
        async fn finish(&self, _: &PendingWrite, _: Vec<NativePart>) -> BlobResult<PutResult> {
            self.committed.store(true, Ordering::SeqCst);
            Err(BlobError::upload_failed(
                "response lost after provider commit",
            ))
        }
        async fn inspect(&self, _: &PendingWrite) -> BlobResult<WriteOutcome> {
            Ok(if self.committed.load(Ordering::SeqCst) {
                WriteOutcome::Committed
            } else if self.aborted.load(Ordering::SeqCst) {
                WriteOutcome::Aborted
            } else {
                WriteOutcome::Uncertain
            })
        }
        async fn abort(&self, _: &PendingWrite) -> BlobResult<()> {
            self.aborted.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    #[tokio::test]
    async fn native_cancellation_and_lost_completion_keep_recoverable_records() {
        for stall in [true, false] {
            let backend = Arc::new(Backend {
                entered: Default::default(),
                committed: AtomicBool::new(false),
                aborted: AtomicBool::new(false),
                stall,
            });
            let journal = Arc::new(MemoryUploadJournal::default());
            let resources = UploadResources::default();
            let staged = crate::bounded::spool_with(
                Box::pin(futures::stream::once(async {
                    Ok(bytes::Bytes::from_static(b"data"))
                })),
                4,
                &resources,
            )
            .await
            .unwrap();
            let b = backend.clone();
            let j = journal.clone();
            let task = tokio::spawn(async move {
                upload(
                    b.as_ref(),
                    j.as_ref(),
                    "key",
                    None,
                    None,
                    staged,
                    &crate::UploadRules::default(),
                )
                .await
            });
            backend.entered.notified().await;
            if stall {
                let record = journal.list().await.unwrap().pop().unwrap();
                assert!(
                    reconcile_write(backend.as_ref(), journal.as_ref(), &record.id)
                        .await
                        .unwrap()
                        .is_none()
                );
                task.abort();
                assert!(task.await.unwrap_err().is_cancelled());
            } else {
                assert!(task.await.unwrap().is_err());
            }
            assert_eq!(resources.usage().staging_bytes, 0);
            let record = journal.list().await.unwrap().pop().unwrap();
            let report = reconcile_write(backend.as_ref(), journal.as_ref(), &record.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                report.outcome,
                if stall {
                    WriteOutcome::Aborted
                } else {
                    WriteOutcome::Committed
                }
            );
            assert_eq!(backend.aborted.load(Ordering::SeqCst), stall);
        }
    }
}
