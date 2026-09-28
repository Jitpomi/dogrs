//! Persist application receipts before acknowledging native upload recovery records.
use std::path::Path;
pub async fn persist(directory: &Path, receipt: &dog_blob::BlobReceipt) -> anyhow::Result<()> {
    let directory = directory.to_owned();
    let name = format!("{}.json", receipt.id);
    let bytes = serde_json::to_vec(receipt)?;
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(&bytes)?;
        file.as_file().sync_all()?;
        file.persist(directory.join(name))?;
        std::fs::File::open(directory)?.sync_all()?;
        Ok(())
    })
    .await?
}
