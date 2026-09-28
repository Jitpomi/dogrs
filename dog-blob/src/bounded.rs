use crate::{BlobCtx, BlobError, BlobPut, BlobResult, ByteStream};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub(crate) fn context(ctx: &BlobCtx) -> BlobResult<()> {
    if ctx.tenant_id.is_empty() || ctx.tenant_id.len() > 512 {
        return Err(BlobError::invalid("tenant must contain 1–512 bytes"));
    }
    Ok(())
}
pub(crate) fn identifier(id: &str) -> BlobResult<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(BlobError::invalid("invalid blob or session identifier"));
    }
    Ok(())
}
pub(crate) fn put_options(put: &BlobPut) -> BlobResult<()> {
    if !put.key_hints.is_empty() || put.idempotency_key.is_some() {
        return Err(BlobError::invalid("per-request key hints and idempotency keys are not supported; use a stable key strategy and application idempotency"));
    }
    Ok(())
}
/// An immutable, privately owned upload validated by DogRS. No public constructor
/// accepts a path. Backends may read path() while borrowing this value; consuming
/// into_stream() retains both the file and its disk reservation until drop.
pub struct ValidatedUpload {
    path: std::path::PathBuf,
    _directory: StagingDirectory,
    _lock: std::fs::File,
    pub(crate) size: u64,
    pub(crate) checksum: String,
    _disk: crate::resources::DiskReservation,
}
struct StagingDirectory(std::path::PathBuf);
impl Drop for StagingDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub(crate) type StagedFile = ValidatedUpload;
impl ValidatedUpload {
    pub fn size_bytes(&self) -> u64 {
        self.size
    }
    pub fn checksum(&self) -> &str {
        &self.checksum
    }
    pub fn into_stream(self) -> ByteStream {
        self.stream()
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub fn stream(self) -> ByteStream {
        Box::pin(async_stream::try_stream! {
            let file = tokio::fs::File::open(&self.path).await?;
            let mut stream = tokio_util::io::ReaderStream::new(file);
            while let Some(chunk) = stream.next().await { yield chunk?; }
            // Keep the private temporary file alive until consumption ends.
            drop(self);
        })
    }
}
/// Bounded disk staging validates the entire input before a backend can commit it.
/// Only the caller's chunk and the buffered file writer are held in memory.
pub(crate) async fn spool_with(
    mut body: ByteStream,
    limit: u64,
    resources: &crate::UploadResources,
) -> BlobResult<StagedFile> {
    let mut disk = resources.disk();
    let directory = tempfile::Builder::new()
        .prefix(".dogrs-building-")
        .tempdir_in(resources.staging_directory())?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(directory.path().join("owner.lock"))?;
    lock.lock()?;
    // Publish the cleanup-visible name only after locking: a concurrent sweeper
    // can never mistake an upload being initialized for an abandoned directory.
    let published = resources
        .staging_directory()
        .join(format!("dogrs-upload-{}", uuid::Uuid::new_v4()));
    std::fs::rename(directory.path(), &published)?;
    let _ = directory.keep(); // Disarm cleanup of the old, unpublished name.
    let directory = StagingDirectory(published);
    let path = directory.0.join("data");
    let file = tokio::fs::File::create(&path).await?;
    let mut writer = tokio::io::BufWriter::new(file);
    let mut size = 0_u64;
    let mut hash = Sha256::new();
    let mut last_progress = tokio::time::Instant::now();
    while let Some(chunk) =
        tokio::time::timeout_at(last_progress + resources.idle_timeout(), body.next())
            .await
            .map_err(|_| BlobError::Timeout {
                operation: "upload stream idle",
            })?
    {
        if last_progress.elapsed() >= resources.idle_timeout() {
            return Err(BlobError::Timeout {
                operation: "upload stream idle",
            });
        }
        tokio::task::consume_budget().await;
        let chunk = chunk?;
        if chunk.is_empty() {
            continue;
        }
        last_progress = tokio::time::Instant::now();
        size = size
            .checked_add(chunk.len() as u64)
            .filter(|n| *n <= limit)
            .ok_or_else(|| BlobError::invalid("upload exceeds byte limit"))?;
        disk.grow(chunk.len() as u64)?;
        writer.write_all(&chunk).await?;
        hash.update(&chunk);
    }
    writer.flush().await?;
    drop(writer);
    Ok(ValidatedUpload {
        _directory: directory,
        _lock: lock,
        _disk: disk,
        path,
        size,
        checksum: format!("sha256:{:x}", hash.finalize()),
    })
}
