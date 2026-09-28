use crate::{BlobError, BlobResult};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// Limits shared by all adapters/coordinators configured with one UploadResources.
#[derive(Debug, Clone)]
pub struct UploadLimits {
    /// Trusted local directory for staging; defaults to the OS temporary directory.
    pub staging_directory: Option<std::path::PathBuf>,
    pub max_concurrent_uploads: usize,
    pub max_staging_bytes: u64,
    pub upload_timeout: Duration,
    pub idle_timeout: Duration,
}
impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            staging_directory: None,
            max_concurrent_uploads: 8,
            max_staging_bytes: 20 * 1024 * 1024 * 1024,
            upload_timeout: Duration::from_secs(900),
            idle_timeout: Duration::from_secs(30),
        }
    }
}
impl UploadLimits {
    pub fn validate(&self) -> BlobResult<()> {
        if self.max_concurrent_uploads == 0
            || self.max_concurrent_uploads > tokio::sync::Semaphore::MAX_PERMITS
            || self.max_staging_bytes == 0
            || self.upload_timeout.is_zero()
            || self.idle_timeout.is_zero()
            || self.upload_timeout > Duration::from_secs(31_536_000)
            || self.idle_timeout > Duration::from_secs(31_536_000)
        {
            return Err(BlobError::invalid("invalid upload resource limits"));
        }
        Ok(())
    }
}
struct State {
    limits: UploadLimits,
    permits: Arc<tokio::sync::Semaphore>,
    bytes: AtomicU64,
    peak: AtomicU64,
    files: AtomicU64,
}
/// Clone this handle to enforce one process-wide budget across components.
#[derive(Clone)]
pub struct UploadResources(Arc<State>);
#[derive(Debug, Clone, Copy)]
pub struct UploadUsage {
    pub staging_bytes: u64,
    pub peak_staging_bytes: u64,
    pub staged_files: u64,
    pub active_uploads: usize,
}
impl Default for UploadResources {
    fn default() -> Self {
        Self::from_limits(UploadLimits::default())
    }
}
impl UploadResources {
    pub fn new(limits: UploadLimits) -> BlobResult<Self> {
        limits.validate()?;
        Ok(Self::from_limits(limits))
    }
    pub(crate) fn from_limits(limits: UploadLimits) -> Self {
        let permits = Arc::new(tokio::sync::Semaphore::new(
            limits
                .max_concurrent_uploads
                .min(tokio::sync::Semaphore::MAX_PERMITS),
        ));
        Self(Arc::new(State {
            limits,
            permits,
            bytes: AtomicU64::new(0),
            peak: AtomicU64::new(0),
            files: AtomicU64::new(0),
        }))
    }
    pub fn usage(&self) -> UploadUsage {
        UploadUsage {
            staging_bytes: self.0.bytes.load(Ordering::SeqCst),
            peak_staging_bytes: self.0.peak.load(Ordering::SeqCst),
            staged_files: self.0.files.load(Ordering::SeqCst),
            active_uploads: self
                .0
                .limits
                .max_concurrent_uploads
                .saturating_sub(self.0.permits.available_permits()),
        }
    }
    pub(crate) fn staging_directory(&self) -> std::path::PathBuf {
        self.0
            .limits
            .staging_directory
            .clone()
            .unwrap_or_else(std::env::temp_dir)
    }
    /// Remove abandoned DogRS staging directories. OS locks exclude live writers,
    /// including other processes. Use only a trusted local filesystem directory.
    pub async fn cleanup_staging(&self) -> BlobResult<usize> {
        let root = self.staging_directory();
        tokio::task::spawn_blocking(move || -> BlobResult<usize> {
            let mut removed = 0;
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                if !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("dogrs-upload-")
                    || !entry.file_type()?.is_dir()
                {
                    continue;
                }
                let lock = match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(entry.path().join("owner.lock"))
                {
                    Ok(f) => f,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                match lock.try_lock() {
                    Ok(()) => match std::fs::remove_dir_all(entry.path()) {
                        Ok(()) => removed += 1,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    },
                    Err(std::fs::TryLockError::WouldBlock) => {}
                    Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
                }
            }
            Ok(removed)
        })
        .await
        .map_err(BlobError::backend)?
    }
    pub(crate) fn idle_timeout(&self) -> Duration {
        self.0.limits.idle_timeout
    }
    pub(crate) async fn run<T>(&self, work: impl Future<Output = BlobResult<T>>) -> BlobResult<T> {
        self.0.limits.validate()?;
        let _permit =
            self.0
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| BlobError::ResourceLimit {
                    resource: "concurrent uploads",
                })?;
        tokio::time::timeout(self.0.limits.upload_timeout, work)
            .await
            .map_err(|_| BlobError::Timeout {
                operation: "upload deadline",
            })?
    }
    pub(crate) fn disk(&self) -> DiskReservation {
        self.0.files.fetch_add(1, Ordering::SeqCst);
        DiskReservation {
            resources: self.clone(),
            bytes: 0,
        }
    }
}
pub(crate) struct DiskReservation {
    resources: UploadResources,
    bytes: u64,
}
impl DiskReservation {
    pub fn grow(&mut self, bytes: u64) -> BlobResult<()> {
        let state = &self.resources.0;
        let previous = state
            .bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                n.checked_add(bytes)
                    .filter(|n| *n <= state.limits.max_staging_bytes)
            })
            .map_err(|_| BlobError::ResourceLimit {
                resource: "staging disk bytes",
            })?;
        self.bytes += bytes;
        state.peak.fetch_max(previous + bytes, Ordering::SeqCst);
        Ok(())
    }
}
impl Drop for DiskReservation {
    fn drop(&mut self) {
        self.resources
            .0
            .bytes
            .fetch_sub(self.bytes, Ordering::SeqCst);
    }
}
