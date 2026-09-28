//! Backend-neutral pending-write journal. A lease excludes a live writer from recovery.
use crate::{BlobError, BlobResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWrite {
    pub id: String,
    pub scope: String,
    pub key: String,
    pub size_bytes: u64,
    pub checksum: String,
    pub content_type: Option<String>,
    pub filename: Option<String>,
    pub native_id: Option<String>,
}
/// Implementations must durably save before returning if restart recovery is promised.
/// A lease must exclude every other writer/reconciler until dropped. list() is only
/// discovery: always acquire a fresh lease before acting. Never unlink a lock inode.
#[async_trait::async_trait]
pub trait UploadJournal: Send + Sync {
    async fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>>;
    async fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>>;
    async fn list(&self) -> BlobResult<Vec<PendingWrite>>;
}
/// After canceling a mutation, drop and reacquire this lease before another
/// mutation. Implementations must retain exclusion until outstanding durable
/// work settles, even if the async caller drops its lease.
#[async_trait::async_trait]
pub trait UploadLease: Send {
    fn record(&self) -> &PendingWrite;
    async fn save(&mut self, record: PendingWrite) -> BlobResult<()>;
    /// Call only after persisting the receipt or resolving the recovery report.
    async fn acknowledge(&mut self) -> BlobResult<()>;
}
#[derive(Default, Clone)]
pub struct MemoryUploadJournal(Arc<Mutex<JournalRecords>>);
type JournalRecords = BTreeMap<String, (PendingWrite, bool)>;
struct MemoryLease {
    acknowledged: bool,
    journal: MemoryUploadJournal,
    record: PendingWrite,
}
impl Drop for MemoryLease {
    fn drop(&mut self) {
        if self.acknowledged {
            return;
        }
        if let Some(v) = self.journal.0.lock().unwrap().get_mut(&self.record.id) {
            v.1 = false;
        }
    }
}
#[async_trait::async_trait]
impl UploadLease for MemoryLease {
    fn record(&self) -> &PendingWrite {
        &self.record
    }
    async fn save(&mut self, record: PendingWrite) -> BlobResult<()> {
        if self.acknowledged {
            return Err(BlobError::invalid("journal lease already acknowledged"));
        }
        unchanged(&self.record, &record)?;
        self.journal
            .0
            .lock()
            .unwrap()
            .insert(record.id.clone(), (record.clone(), true));
        self.record = record;
        Ok(())
    }
    async fn acknowledge(&mut self) -> BlobResult<()> {
        self.journal.0.lock().unwrap().remove(&self.record.id);
        self.acknowledged = true;
        Ok(())
    }
}
fn unchanged(old: &PendingWrite, next: &PendingWrite) -> BlobResult<()> {
    if old.id != next.id
        || old.scope != next.scope
        || old.key != next.key
        || old.size_bytes != next.size_bytes
        || old.checksum != next.checksum
        || old.content_type != next.content_type
        || old.filename != next.filename
        || (old.native_id.is_some() && old.native_id != next.native_id)
    {
        return Err(BlobError::invalid("pending write identity changed"));
    }
    Ok(())
}
#[async_trait::async_trait]
impl UploadJournal for MemoryUploadJournal {
    async fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>> {
        let mut records = self.0.lock().unwrap();
        if records.len() >= 1024 || records.contains_key(&record.id) {
            return Err(BlobError::ResourceLimit {
                resource: "pending write journal",
            });
        }
        records.insert(record.id.clone(), (record.clone(), true));
        Ok(Box::new(MemoryLease {
            acknowledged: false,
            journal: self.clone(),
            record,
        }))
    }
    async fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>> {
        let mut records = self.0.lock().unwrap();
        let Some((record, busy)) = records.get_mut(id) else {
            return Ok(None);
        };
        if *busy {
            return Ok(None);
        }
        *busy = true;
        Ok(Some(Box::new(MemoryLease {
            acknowledged: false,
            journal: self.clone(),
            record: record.clone(),
        })))
    }
    async fn list(&self) -> BlobResult<Vec<PendingWrite>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .map(|(r, _)| r.clone())
            .collect())
    }
}
/// Durable journal in a trusted application-owned directory on a local filesystem.
/// OS locks protect live writers across processes; atomic rename + fsync protects
/// records. All processes must share the same directory. Network filesystems whose
/// lock/rename/fsync semantics differ are not supported by this implementation.
#[derive(Clone)]
pub struct FileUploadJournal {
    directory: PathBuf,
}
struct FileLease {
    in_flight: bool,
    acknowledged: bool,
    _lock: File,
    path: PathBuf,
    directory: PathBuf,
    record: PendingWrite,
}
impl FileUploadJournal {
    pub fn new(directory: impl AsRef<Path>) -> BlobResult<Self> {
        let directory = directory.as_ref();
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(directory)?;
        Ok(Self {
            directory: directory.canonicalize()?,
        })
    }
    fn lock(&self, id: &str) -> BlobResult<Option<File>> {
        crate::bounded::identifier(id)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(self.directory.join(format!("{id}.lock")))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }
}
fn persist_record(path: &Path, directory: &Path, record: &PendingWrite) -> BlobResult<()> {
    let mut temp = tempfile::NamedTempFile::new_in(directory)?;
    temp.write_all(&serde_json::to_vec(record)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    File::open(directory)?.sync_all()?;
    Ok(())
}
#[async_trait::async_trait]
impl UploadLease for FileLease {
    fn record(&self) -> &PendingWrite {
        &self.record
    }
    async fn save(&mut self, record: PendingWrite) -> BlobResult<()> {
        if self.acknowledged || self.in_flight {
            return Err(BlobError::invalid("journal lease already acknowledged"));
        }
        unchanged(&self.record, &record)?;
        let path = self.path.clone();
        let directory = self.directory.clone();
        let value = record.clone();
        let lock = self._lock.try_clone()?;
        // Retain a duplicate lock descriptor in the blocking operation even if
        // the async caller is canceled, so recovery cannot race a late rename.
        self.in_flight = true;
        tokio::task::spawn_blocking(move || {
            let _lock = lock;
            persist_record(&path, &directory, &value)
        })
        .await
        .map_err(BlobError::backend)??;
        self.in_flight = false;
        self.record = record;
        Ok(())
    }
    async fn acknowledge(&mut self) -> BlobResult<()> {
        if self.in_flight {
            return Err(BlobError::invalid(
                "canceled journal lease must be reacquired",
            ));
        }
        let path = self.path.clone();
        let directory = self.directory.clone();
        let lock = self._lock.try_clone()?;
        self.in_flight = true;
        tokio::task::spawn_blocking(move || -> BlobResult<()> {
            let _lock = lock;
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            File::open(directory)?.sync_all()?;
            Ok(())
        })
        .await
        .map_err(BlobError::backend)??;
        self.in_flight = false;
        self.acknowledged = true;
        Ok(())
    }
}
impl FileUploadJournal {
    fn create_sync(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>> {
        let lock = self
            .lock(&record.id)?
            .ok_or_else(|| BlobError::invalid("write already leased"))?;
        let path = self.directory.join(format!("{}.json", record.id));
        if path.exists() {
            return Err(BlobError::invalid("write already recorded"));
        }
        persist_record(&path, &self.directory, &record)?;
        Ok(Box::new(FileLease {
            in_flight: false,
            acknowledged: false,
            _lock: lock,
            path,
            directory: self.directory.clone(),
            record,
        }))
    }
    fn acquire_sync(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>> {
        let Some(lock) = self.lock(id)? else {
            return Ok(None);
        };
        let path = self.directory.join(format!("{id}.json"));
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let record: PendingWrite = serde_json::from_slice(&bytes)?;
        if record.id != id {
            return Err(BlobError::invalid("journal identity mismatch"));
        }
        Ok(Some(Box::new(FileLease {
            in_flight: false,
            acknowledged: false,
            _lock: lock,
            path,
            directory: self.directory.clone(),
            record,
        })))
    }
    fn list_sync(&self) -> BlobResult<Vec<PendingWrite>> {
        let mut records = Vec::new();
        for entry in std::fs::read_dir(&self.directory)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                match std::fs::read(path) {
                    Ok(bytes) => records.push(serde_json::from_slice(&bytes)?),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(records)
    }
}
#[async_trait::async_trait]
impl UploadJournal for FileUploadJournal {
    async fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.create_sync(record))
            .await
            .map_err(BlobError::backend)?
    }
    async fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>> {
        let journal = self.clone();
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || journal.acquire_sync(&id))
            .await
            .map_err(BlobError::backend)?
    }
    async fn list(&self) -> BlobResult<Vec<PendingWrite>> {
        let journal = self.clone();
        tokio::task::spawn_blocking(move || journal.list_sync())
            .await
            .map_err(BlobError::backend)?
    }
}
