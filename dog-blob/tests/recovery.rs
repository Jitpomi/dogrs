use async_trait::async_trait;
use dog_blob::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
fn record(id: &str) -> PendingWrite {
    PendingWrite {
        id: id.into(),
        scope: "test-store".into(),
        key: format!("tenant/{id}"),
        size_bytes: 4,
        checksum: "sha256:test".into(),
        content_type: None,
        filename: None,
        native_id: Some("provider-handle".into()),
    }
}
struct Native {
    committed: bool,
    aborted: AtomicUsize,
}
#[async_trait]
impl NativeMultipartStore for Native {
    fn recovery_scope(&self) -> String {
        "test-store".into()
    }
    fn minimum_part_size(&self) -> u64 {
        1
    }
    async fn initiate(&self, _: &PendingWrite) -> BlobResult<String> {
        unreachable!()
    }
    async fn upload_part(
        &self,
        _: &PendingWrite,
        _: u32,
        _: &ValidatedUpload,
        _: u64,
        _: u64,
    ) -> BlobResult<String> {
        unreachable!()
    }
    async fn finish(&self, _: &PendingWrite, _: Vec<NativePart>) -> BlobResult<PutResult> {
        unreachable!()
    }
    async fn inspect(&self, _: &PendingWrite) -> BlobResult<WriteOutcome> {
        Ok(if self.committed {
            WriteOutcome::Committed
        } else if self.aborted.load(Ordering::SeqCst) > 0 {
            WriteOutcome::Aborted
        } else {
            WriteOutcome::Uncertain
        })
    }
    async fn abort(&self, _: &PendingWrite) -> BlobResult<()> {
        self.aborted.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn journal_leases_protect_live_writes_and_preserve_uncertainty() {
    let dir = tempfile::tempdir().unwrap();
    for journal in [
        Arc::new(MemoryUploadJournal::default()) as Arc<dyn UploadJournal>,
        Arc::new(FileUploadJournal::new(dir.path()).unwrap()),
    ] {
        let lease = journal.create(record("live")).await.unwrap();
        let store = Native {
            committed: false,
            aborted: AtomicUsize::new(0),
        };
        assert!(reconcile_write(&store, journal.as_ref(), "live")
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.aborted.load(Ordering::SeqCst), 0);
        drop(lease);
        assert_eq!(
            reconcile_write(&store, journal.as_ref(), "live")
                .await
                .unwrap()
                .unwrap()
                .outcome,
            WriteOutcome::Aborted
        );
        journal
            .acquire("live")
            .await
            .unwrap()
            .unwrap()
            .acknowledge()
            .await
            .unwrap();
        assert!(journal.list().await.unwrap().is_empty());
        let mut unknown = record("unknown");
        unknown.native_id = None;
        drop(journal.create(unknown).await.unwrap());
        let store = Native {
            committed: false,
            aborted: AtomicUsize::new(0),
        };
        assert_eq!(
            reconcile_write(&store, journal.as_ref(), "unknown")
                .await
                .unwrap()
                .unwrap()
                .outcome,
            WriteOutcome::Uncertain
        );
        assert_eq!(store.aborted.load(Ordering::SeqCst), 0);
        let store = Native {
            committed: true,
            aborted: AtomicUsize::new(0),
        };
        assert_eq!(
            reconcile_write(&store, journal.as_ref(), "unknown")
                .await
                .unwrap()
                .unwrap()
                .outcome,
            WriteOutcome::Committed
        );
        assert_eq!(store.aborted.load(Ordering::SeqCst), 0);
        let mut lease = journal.acquire("unknown").await.unwrap().unwrap();
        let mut changed = lease.record().clone();
        changed.key = "another-tenant".into();
        assert!(lease.save(changed).await.is_err());
    }
}
#[tokio::test]
async fn journal_crash_child() {
    let Ok(path) = std::env::var("DOGRS_JOURNAL_CRASH_TEST") else {
        return;
    };
    let journal = FileUploadJournal::new(&path).unwrap();
    let _lease = journal.create(record("crashed")).await.unwrap();
    std::fs::write(std::path::Path::new(&path).join("ready"), b"ready").unwrap();
    loop {
        std::thread::park();
    }
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
async fn durable_journal_recovers_after_process_kill_and_excludes_live_process() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "journal_crash_child", "--nocapture"])
            .env("DOGRS_JOURNAL_CRASH_TEST", dir.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while !dir.path().join("ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let journal = FileUploadJournal::new(dir.path()).unwrap();
    assert!(journal.acquire("crashed").await.unwrap().is_none());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let store = Native {
        committed: false,
        aborted: AtomicUsize::new(0),
    };
    let report = reconcile_write(&store, &journal, "crashed")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(report.record.native_id.as_deref(), Some("provider-handle"));
    assert_eq!(report.outcome, WriteOutcome::Aborted);
}

#[test]
fn canceled_disk_journal_mutation_cannot_be_reused_or_race_recovery() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let journal = FileUploadJournal::new(directory.path()).unwrap();
        let mut initial = record("queued-save");
        initial.native_id = None;
        let mut lease = journal.create(initial.clone()).await.unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = tokio::task::spawn_blocking(move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        let mut next = initial;
        next.native_id = Some("first-handle".into());
        let mut saving = lease.save(next.clone());
        assert!(futures::poll!(saving.as_mut()).is_pending());
        drop(saving); // Its disk worker is queued and still owns a duplicate lock.
        assert!(lease.save(next.clone()).await.is_err());
        assert!(lease.acknowledge().await.is_err());
        drop(lease);
        // Direct OS probe cannot acquire while the canceled worker retains its lock.
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(directory.path().join("queued-save.lock"))
            .unwrap();
        assert!(matches!(
            file.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        release_tx.send(()).unwrap();
        worker.await.unwrap();
        let recovered = journal.acquire("queued-save").await.unwrap().unwrap();
        assert_eq!(recovered.record().native_id, next.native_id);
    });
}
