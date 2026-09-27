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
pub(crate) struct StagedFile {
    path: tempfile::TempPath,
    pub size: u64,
    pub checksum: String,
}
impl StagedFile {
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
pub(crate) async fn spool(mut body: ByteStream, limit: u64) -> BlobResult<StagedFile> {
    let temp = tempfile::NamedTempFile::new()?;
    let file = tokio::fs::File::from_std(temp.reopen()?);
    let path = temp.into_temp_path();
    let mut writer = tokio::io::BufWriter::new(file);
    let mut size = 0_u64;
    let mut hash = Sha256::new();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        size = size
            .checked_add(chunk.len() as u64)
            .filter(|n| *n <= limit)
            .ok_or_else(|| BlobError::invalid("upload exceeds byte limit"))?;
        writer.write_all(&chunk).await?;
        hash.update(&chunk);
    }
    writer.flush().await?;
    drop(writer);
    Ok(StagedFile {
        path,
        size,
        checksum: format!("sha256:{:x}", hash.finalize()),
    })
}
