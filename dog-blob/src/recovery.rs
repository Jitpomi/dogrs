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
pub trait UploadJournal: Send + Sync {
    fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>>;
    fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>>;
    fn list(&self) -> BlobResult<Vec<PendingWrite>>;
}
pub trait UploadLease: Send {
    fn record(&self) -> &PendingWrite;
    fn save(&mut self, record: PendingWrite) -> BlobResult<()>;
    /// Call only after persisting the receipt or resolving the recovery report.
    fn acknowledge(&mut self) -> BlobResult<()>;
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
impl UploadLease for MemoryLease {
    fn record(&self) -> &PendingWrite {
        &self.record
    }
    fn save(&mut self, record: PendingWrite) -> BlobResult<()> {
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
    fn acknowledge(&mut self) -> BlobResult<()> {
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
impl UploadJournal for MemoryUploadJournal {
    fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>> {
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
    fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>> {
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
    fn list(&self) -> BlobResult<Vec<PendingWrite>> {
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
impl UploadLease for FileLease {
    fn record(&self) -> &PendingWrite {
        &self.record
    }
    fn save(&mut self, record: PendingWrite) -> BlobResult<()> {
        if self.acknowledged {
            return Err(BlobError::invalid("journal lease already acknowledged"));
        }
        unchanged(&self.record, &record)?;
        let mut temp = tempfile::NamedTempFile::new_in(&self.directory)?;
        temp.write_all(&serde_json::to_vec(&record)?)?;
        temp.as_file().sync_all()?;
        temp.persist(&self.path).map_err(|e| e.error)?;
        File::open(&self.directory)?.sync_all()?;
        self.record = record;
        Ok(())
    }
    fn acknowledge(&mut self) -> BlobResult<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        File::open(&self.directory)?.sync_all()?;
        self.acknowledged = true;
        Ok(())
    }
}
impl UploadJournal for FileUploadJournal {
    fn create(&self, record: PendingWrite) -> BlobResult<Box<dyn UploadLease>> {
        let lock = self
            .lock(&record.id)?
            .ok_or_else(|| BlobError::invalid("write already leased"))?;
        let path = self.directory.join(format!("{}.json", record.id));
        if path.exists() {
            return Err(BlobError::invalid("write already recorded"));
        }
        let mut lease = FileLease {
            acknowledged: false,
            _lock: lock,
            path,
            directory: self.directory.clone(),
            record: record.clone(),
        };
        lease.save(record)?;
        Ok(Box::new(lease))
    }
    fn acquire(&self, id: &str) -> BlobResult<Option<Box<dyn UploadLease>>> {
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
            acknowledged: false,
            _lock: lock,
            path,
            directory: self.directory.clone(),
            record,
        })))
    }
    fn list(&self) -> BlobResult<Vec<PendingWrite>> {
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
