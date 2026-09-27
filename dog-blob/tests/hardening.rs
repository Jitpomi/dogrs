use async_trait::async_trait;
use dog_blob::*;
use futures_util::StreamExt;
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

#[derive(Clone, Default)]
struct Store {
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    part_gate: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    final_gate: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    final_started: Arc<tokio::sync::Notify>,
    fail_final: Arc<AtomicBool>,
}
fn body(bytes: &[u8]) -> ByteStream {
    let bytes = bytes::Bytes::copy_from_slice(bytes);
    Box::pin(futures::stream::once(async { Ok(bytes) }))
}
#[async_trait]
impl BlobStore for Store {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn put(
        &self,
        key: &str,
        _: Option<&str>,
        mut stream: ByteStream,
    ) -> BlobResult<PutResult> {
        let gate = if key.starts_with("__uploads/") {
            self.part_gate.lock().unwrap().clone()
        } else {
            self.final_gate.lock().unwrap().clone()
        };
        if !key.starts_with("__uploads/") {
            self.final_started.notify_one();
        }
        if let Some(gate) = gate {
            gate.wait().await;
        }
        if !key.starts_with("__uploads/") && self.fail_final.load(Ordering::SeqCst) {
            return Err(BlobError::upload_failed("fixture failure"));
        }
        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            data.extend_from_slice(&chunk?);
        }
        let size_bytes = data.len() as u64;
        self.objects.lock().unwrap().insert(key.into(), data);
        Ok(PutResult {
            size_bytes,
            etag: None,
            checksum: None,
        })
    }
    async fn get(&self, key: &str, _: Option<ByteRange>) -> BlobResult<GetResult> {
        let bytes = self
            .objects
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or_else(|| BlobError::not_found("fixture"))?;
        Ok(GetResult {
            size_bytes: bytes.len() as u64,
            stream: body(&bytes),
            content_type: None,
            etag: None,
            resolved_range: None,
        })
    }
    async fn head(&self, key: &str) -> BlobResult<ObjectHead> {
        let data = self.objects.lock().unwrap();
        let bytes = data
            .get(key)
            .ok_or_else(|| BlobError::not_found("fixture"))?;
        Ok(ObjectHead {
            size_bytes: bytes.len() as u64,
            content_type: None,
            etag: None,
            last_modified: None,
        })
    }
    async fn delete(&self, key: &str) -> BlobResult<()> {
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }
    async fn list(&self, prefix: Option<&str>, _: Option<usize>) -> BlobResult<Vec<BlobInfo>> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .iter()
            .filter(|(k, _)| k.starts_with(prefix.unwrap_or("")))
            .map(|(key, data)| BlobInfo {
                key: key.clone(),
                size_bytes: data.len() as u64,
                content_type: None,
                filename: None,
                etag: None,
                last_modified: None,
                metadata: Default::default(),
            })
            .collect())
    }
    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities::basic()
    }
}
fn config() -> BlobConfig {
    BlobConfig {
        max_blob_bytes: 8,
        upload_rules: UploadRules {
            part_size: 4,
            max_parts: 4,
            require_fixed_part_size: true,
            allow_out_of_order: true,
        },
        ..BlobConfig::default()
    }
}
fn ctx(tenant: &str) -> BlobCtx {
    BlobCtx::new(tenant.into()).with_actor("owner".into())
}
fn coordinator(store: Store, sessions: MemoryUploadSessionStore) -> DefaultUploadCoordinator {
    DefaultUploadCoordinator::new(
        store,
        sessions,
        DefaultKeyStrategy,
        config().with_checksum("sha256"),
    )
}
async fn begin(c: &DefaultUploadCoordinator, total: u32) -> UploadSession {
    let id = BlobId::new();
    let key = DefaultKeyStrategy.object_key("a", id.as_str(), &Default::default());
    c.begin(
        ctx("a"),
        UploadIntent::new(id, key).with_parts(4, Some(total)),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn actual_size_and_stream_errors_never_reach_storage() {
    let store = Store::default();
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(store.clone(), config())));
    assert!(adapter
        .put(ctx("a"), BlobPut::new(), body(b"123456789"))
        .await
        .is_err());
    assert!(adapter
        .put(ctx("a"), BlobPut::new().with_size_hint(2), body(b"123"))
        .await
        .is_err());
    let stream = Box::pin(futures::stream::iter([
        Ok(bytes::Bytes::from_static(b"ok")),
        Err(std::io::Error::other("fixture")),
    ]));
    assert!(adapter.put(ctx("a"), BlobPut::new(), stream).await.is_err());
    assert!(store.objects.lock().unwrap().is_empty());
}
#[tokio::test]
async fn json_paths_are_rejected_without_reading_or_deleting_files() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"private").unwrap();
    assert!(
        BlobAdapter::extract_file_data(&serde_json::json!({"file":{"temp_path":file.path()}}))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(file.path()).unwrap(), b"private");
    assert_eq!(
        BlobAdapter::extract_file_data(&serde_json::json!({"file":"aGk="}))
            .await
            .unwrap(),
        b"hi"
    );
}
#[tokio::test]
async fn stable_keys_listing_and_read_isolation() {
    let store = Store::default();
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(store.clone(), config())));
    let a = adapter
        .put(ctx("a"), BlobPut::new(), body(b"one"))
        .await
        .unwrap();
    adapter
        .put(ctx("alice"), BlobPut::new(), body(b"two"))
        .await
        .unwrap();
    assert_eq!(a.key, format!("v2/61/{}", a.id));
    assert_eq!(adapter.list(ctx("a"), None, None).await.unwrap().len(), 1);
    assert!(adapter
        .open(ctx("alice"), a.id.clone(), None)
        .await
        .is_err());
    assert!(adapter
        .open(ctx("a"), BlobId::from_string("../../alice".into()), None)
        .await
        .is_err());
    assert!(adapter.open(ctx("a"), a.id.clone(), None).await.is_ok());
    let strict = BlobAdapter::new(Arc::new(BlobState::new(
        store,
        config().require_range_support(),
    )));
    assert!(strict
        .open(ctx("a"), a.id, Some(ByteRange::from_start(0)))
        .await
        .is_err());
}
#[tokio::test]
async fn every_multipart_operation_checks_tenant_and_actor() {
    let c = coordinator(Store::default(), MemoryUploadSessionStore::new());
    let s = begin(&c, 1).await;
    for foreign in [
        ctx("b"),
        ctx("a").with_actor("other".into()),
        BlobCtx::new("a".into()),
    ] {
        assert!(c.get_session(foreign.clone(), &s.upload_id).await.is_err());
        assert!(c
            .accept_part(foreign.clone(), &s.upload_id, 1, body(b"hi"))
            .await
            .is_err());
        assert!(c
            .set_total_parts(foreign.clone(), &s.upload_id, 1)
            .await
            .is_err());
        assert!(c.complete(foreign.clone(), &s.upload_id).await.is_err());
        assert!(c.abort(foreign.clone(), &s.upload_id).await.is_err());
        assert!(c.forget(foreign, &s.upload_id).await.is_err());
    }
    assert_eq!(
        c.get_session(ctx("a"), &s.upload_id).await.unwrap().status,
        UploadStatus::Active
    );
}
#[tokio::test]
async fn multipart_limits_completion_retries_and_cleanup() {
    let store = Store::default();
    let c = coordinator(store.clone(), MemoryUploadSessionStore::new());
    let s = begin(&c, 2).await;
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 0, body(b"1234"))
        .await
        .is_err());
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"12"))
        .await
        .is_err());
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 3, body(b"1234"))
        .await
        .is_err());
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"12345"))
        .await
        .is_err());
    c.accept_part(ctx("a"), &s.upload_id, 1, body(b"1234"))
        .await
        .unwrap();
    assert!(c.complete(ctx("a"), &s.upload_id).await.is_err());
    c.accept_part(ctx("a"), &s.upload_id, 2, body(b"56"))
        .await
        .unwrap();
    let receipt = c.complete(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"123456");
    let retried = c.complete(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(receipt.checksum, retried.checksum);
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"xxxx"))
        .await
        .is_err());
    assert!(c.abort(ctx("a"), &s.upload_id).await.is_err());
    c.forget(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(store.objects.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn concurrent_coordinators_cannot_lose_part_updates() {
    let store = Store::default();
    let sessions = MemoryUploadSessionStore::new();
    let a = coordinator(store.clone(), sessions.clone());
    let b = coordinator(store.clone(), sessions);
    let s = begin(&a, 2).await;
    *store.part_gate.lock().unwrap() = Some(Arc::new(tokio::sync::Barrier::new(2)));
    let (one, two) = tokio::join!(
        a.accept_part(ctx("a"), &s.upload_id, 1, body(b"1234")),
        b.accept_part(ctx("a"), &s.upload_id, 2, body(b"5678"))
    );
    assert_ne!(one.is_ok(), two.is_ok());
    *store.part_gate.lock().unwrap() = None;
    let number = if one.is_err() { 1 } else { 2 };
    let bytes = if number == 1 { b"1234" } else { b"5678" };
    a.accept_part(ctx("a"), &s.upload_id, number, body(bytes))
        .await
        .unwrap();
    let receipt = b.complete(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"12345678");
}
#[tokio::test]
async fn canceled_completion_resumes_from_immutable_manifest() {
    let store = Store::default();
    let sessions = MemoryUploadSessionStore::new();
    let c = Arc::new(coordinator(store.clone(), sessions.clone()));
    let s = begin(&c, 1).await;
    c.accept_part(ctx("a"), &s.upload_id, 1, body(b"data"))
        .await
        .unwrap();
    *store.final_gate.lock().unwrap() = Some(Arc::new(tokio::sync::Barrier::new(2)));
    let id = s.upload_id.clone();
    let running = c.clone();
    let task = tokio::spawn(async move { running.complete(ctx("a"), &id).await });
    store.final_started.notified().await;
    task.abort();
    let _ = task.await;
    assert_eq!(
        c.get_session(ctx("a"), &s.upload_id).await.unwrap().status,
        UploadStatus::Completing
    );
    assert!(c.abort(ctx("a"), &s.upload_id).await.is_err());
    *store.final_gate.lock().unwrap() = None;
    let restarted = coordinator(store.clone(), sessions);
    let receipt = restarted.complete(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"data");
}
#[tokio::test]
async fn corrupt_parts_never_commit_final_object() {
    let store = Store::default();
    let c = coordinator(store.clone(), MemoryUploadSessionStore::new());
    let s = begin(&c, 1).await;
    let part = c
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"data"))
        .await
        .unwrap();
    store
        .objects
        .lock()
        .unwrap()
        .insert(part.storage_key.clone(), b"evil".to_vec());
    assert!(c.complete(ctx("a"), &s.upload_id).await.is_err());
    assert!(!store.objects.lock().unwrap().contains_key(&s.object_key));
    store
        .objects
        .lock()
        .unwrap()
        .insert(part.storage_key, b"data".to_vec());
    assert!(c.complete(ctx("a"), &s.upload_id).await.is_ok());
}
#[tokio::test]
async fn chunk_ownership_retries_metadata_and_numeric_validation() {
    let store = Store::default();
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(store.clone(), config())));
    let id = ChunkSessionId::new();
    adapter
        .put_chunk(ctx("a"), id.clone(), 0, 2, BlobPut::new(), b"1234".to_vec())
        .await
        .unwrap();
    assert!(adapter
        .put_chunk(ctx("b"), id.clone(), 1, 2, BlobPut::new(), b"5".to_vec())
        .await
        .is_err());
    assert!(adapter
        .put_chunk(ctx("a"), id.clone(), 1, 3, BlobPut::new(), b"5678".to_vec())
        .await
        .is_err());
    let complete = adapter
        .put_chunk(ctx("a"), id.clone(), 1, 2, BlobPut::new(), b"56".to_vec())
        .await
        .unwrap();
    let ChunkResult::Complete { receipt } = complete else {
        panic!("not complete")
    };
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"123456");
    let retry = adapter
        .put_chunk(ctx("a"), id.clone(), 1, 2, BlobPut::new(), b"56".to_vec())
        .await
        .unwrap();
    let ChunkResult::Complete { receipt: retry } = retry else {
        panic!("not complete")
    };
    assert_eq!(receipt.id, retry.id);
    assert!(adapter
        .put_chunk(ctx("a"), id.clone(), 1, 2, BlobPut::new(), b"xx".to_vec())
        .await
        .is_err());
    assert!(adapter.forget_chunk(ctx("b"), id.clone()).await.is_err());
    adapter.forget_chunk(ctx("a"), id).await.unwrap();
    assert!(adapter.put_from_multipart(ctx("a"),&serde_json::json!({"file":"aGk=","dzuuid":"../escape","dzchunkindex":0,"dztotalchunkcount":1})).await.is_err());
    assert!(adapter.put_from_multipart(ctx("a"),&serde_json::json!({"file":"aGk=","dzuuid":"ok","dzchunkindex":4294967296_u64,"dztotalchunkcount":1})).await.is_err());
    assert!(adapter
        .put_from_multipart(ctx("a"), &serde_json::json!({"file":"aGk=","dzuuid":"ok"}))
        .await
        .is_err());
}
#[tokio::test]
async fn configuration_and_session_capacity_fail_closed() {
    let mut bad = config();
    bad.upload_rules.part_size = 0;
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(Store::default(), bad)));
    assert!(adapter
        .begin_multipart(ctx("a"), BlobPut::new().with_size_hint(1))
        .await
        .is_err());
    assert!(config().with_checksum("typo").validate().is_err());
    let c = coordinator(Store::default(), MemoryUploadSessionStore::with_capacity(1));
    let first = begin(&c, 1).await;
    let id = BlobId::new();
    let key = DefaultKeyStrategy.object_key("a", id.as_str(), &Default::default());
    assert!(c
        .begin(ctx("a"), UploadIntent::new(id, key).with_parts(4, Some(1)))
        .await
        .is_err());
    c.abort(ctx("a"), &first.upload_id).await.unwrap();
    c.forget(ctx("a"), &first.upload_id).await.unwrap();
    begin(&c, 1).await;
}
#[tokio::test]
async fn automatic_multipart_and_trusted_file_handle() {
    let store = Store::default();
    let mut conf = config();
    conf.multipart_threshold_bytes = 4;
    let c = coordinator(store.clone(), MemoryUploadSessionStore::new());
    let adapter = BlobAdapter::new(Arc::new(
        BlobState::new(store.clone(), conf).with_uploads(c),
    ));
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), b"123456").unwrap();
    let receipt = adapter
        .put_file(
            ctx("a"),
            BlobPut::new(),
            tokio::fs::File::open(temp.path()).await.unwrap(),
        )
        .await
        .unwrap();
    assert!(temp.path().exists());
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"123456");
    let UploadInfo::Multipart { upload_id, .. } = receipt.upload else {
        panic!("multipart not used")
    };
    adapter.forget_upload(ctx("a"), upload_id).await.unwrap();
}

#[tokio::test]
async fn backend_failure_and_concurrent_completion_preserve_frozen_data() {
    let store = Store::default();
    let sessions = MemoryUploadSessionStore::new();
    let a = coordinator(store.clone(), sessions.clone());
    let b = coordinator(store.clone(), sessions);
    let s = begin(&a, 1).await;
    a.accept_part(ctx("a"), &s.upload_id, 1, body(b"data"))
        .await
        .unwrap();
    store.fail_final.store(true, Ordering::SeqCst);
    assert!(a.complete(ctx("a"), &s.upload_id).await.is_err());
    assert!(!store.objects.lock().unwrap().contains_key(&s.object_key));
    assert!(a
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"evil"))
        .await
        .is_err());
    store.fail_final.store(false, Ordering::SeqCst);
    *store.final_gate.lock().unwrap() = Some(Arc::new(tokio::sync::Barrier::new(2)));
    let (one, two) = tokio::join!(
        a.complete(ctx("a"), &s.upload_id),
        b.complete(ctx("a"), &s.upload_id)
    );
    let one = one.unwrap();
    let two = two.unwrap();
    assert_eq!(one.id, two.id);
    assert_eq!(one.checksum, two.checksum);
    assert_eq!(store.objects.lock().unwrap()[&one.key], b"data");
}

#[tokio::test]
async fn expiry_releases_chunk_capacity_and_allows_multipart_abort_cleanup() {
    let store = Store::default();
    let mut cfg = config();
    cfg.session_ttl_secs = 2;
    cfg.max_chunk_sessions = 1;
    let sessions = MemoryUploadSessionStore::new();
    let c = DefaultUploadCoordinator::new(
        store.clone(),
        sessions.clone(),
        DefaultKeyStrategy,
        cfg.clone(),
    );
    let s = begin(&c, 1).await;
    c.accept_part(ctx("a"), &s.upload_id, 1, body(b"data"))
        .await
        .unwrap();
    let adapter = BlobAdapter::new(Arc::new(BlobState::new(store.clone(), cfg)));
    adapter
        .put_chunk(
            ctx("a"),
            ChunkSessionId::new(),
            0,
            2,
            BlobPut::new(),
            b"1234".to_vec(),
        )
        .await
        .unwrap();
    assert!(adapter
        .put_chunk(
            ctx("a"),
            ChunkSessionId::new(),
            0,
            2,
            BlobPut::new(),
            b"1234".to_vec()
        )
        .await
        .is_err());
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    assert_eq!(sessions.expired().len(), 1);
    assert!(c.complete(ctx("a"), &s.upload_id).await.is_err());
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 1, body(b"data"))
        .await
        .is_err());
    c.abort(ctx("a"), &s.upload_id).await.unwrap();
    c.forget(ctx("a"), &s.upload_id).await.unwrap();
    assert!(store.objects.lock().unwrap().is_empty());
    assert_eq!(adapter.cleanup_expired_chunks().await, 1);
    assert_eq!(adapter.cleanup_expired_chunks().await, 0);
    adapter
        .put_chunk(
            ctx("a"),
            ChunkSessionId::new(),
            0,
            2,
            BlobPut::new(),
            b"1234".to_vec(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn ordered_parts_aggregate_limit_and_replacement_accounting() {
    let store = Store::default();
    let mut cfg = config();
    cfg.upload_rules.allow_out_of_order = false;
    let c = DefaultUploadCoordinator::new(
        store.clone(),
        MemoryUploadSessionStore::new(),
        DefaultKeyStrategy,
        cfg,
    );
    let s = begin(&c, 3).await;
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 2, body(b"1234"))
        .await
        .is_err());
    c.accept_part(ctx("a"), &s.upload_id, 1, body(b"1234"))
        .await
        .unwrap();
    c.accept_part(ctx("a"), &s.upload_id, 1, body(b"abcd"))
        .await
        .unwrap();
    c.accept_part(ctx("a"), &s.upload_id, 2, body(b"5678"))
        .await
        .unwrap();
    assert!(c
        .accept_part(ctx("a"), &s.upload_id, 3, body(b"9"))
        .await
        .is_err());
    c.set_total_parts(ctx("a"), &s.upload_id, 2).await.unwrap();
    let receipt = c.complete(ctx("a"), &s.upload_id).await.unwrap();
    assert_eq!(store.objects.lock().unwrap()[&receipt.key], b"abcd5678");
}

struct LegacySessions(MemoryUploadSessionStore);
#[async_trait]
impl UploadSessionStore for LegacySessions {
    async fn create(&self, s: UploadSession) -> BlobResult<UploadSession> {
        self.0.create(s).await
    }
    async fn get(&self, id: &UploadId) -> BlobResult<UploadSession> {
        self.0.get(id).await
    }
    async fn update(&self, s: UploadSession) -> BlobResult<UploadSession> {
        self.0.update(s).await
    }
    async fn delete(&self, id: &UploadId) -> BlobResult<()> {
        self.0.delete(id).await
    }
}
#[tokio::test]
async fn legacy_nonatomic_session_store_is_rejected_before_upload() {
    let store = Store::default();
    let c = DefaultUploadCoordinator::new(
        store.clone(),
        LegacySessions(MemoryUploadSessionStore::new()),
        DefaultKeyStrategy,
        config(),
    );
    let id = BlobId::new();
    let key = DefaultKeyStrategy.object_key("a", id.as_str(), &Default::default());
    assert!(matches!(
        c.begin(ctx("a"), UploadIntent::new(id, key).with_parts(4, Some(1)))
            .await,
        Err(BlobError::Unsupported)
    ));
    assert!(store.objects.lock().unwrap().is_empty());
}
