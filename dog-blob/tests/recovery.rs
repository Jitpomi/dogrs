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
        let lease = journal.create(record("live")).unwrap();
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
            .unwrap()
            .unwrap()
            .acknowledge()
            .unwrap();
        assert!(journal.list().unwrap().is_empty());
        let mut unknown = record("unknown");
        unknown.native_id = None;
        drop(journal.create(unknown).unwrap());
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
        let mut lease = journal.acquire("unknown").unwrap().unwrap();
        let mut changed = lease.record().clone();
        changed.key = "another-tenant".into();
        assert!(lease.save(changed).is_err());
    }
}
#[test]
fn journal_crash_child() {
    let Ok(path) = std::env::var("DOGRS_JOURNAL_CRASH_TEST") else {
        return;
    };
    let journal = FileUploadJournal::new(&path).unwrap();
    let _lease = journal.create(record("crashed")).unwrap();
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
    assert!(journal.acquire("crashed").unwrap().is_none());
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
