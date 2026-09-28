//! Durable revocation example for processes sharing a trusted local filesystem.
//! Entries are intentionally retained. Archive only offline after every issued token expires.
//! Distributed hosts should implement TokenStore using a shared transactional database.
use anyhow::Result;
use dog_auth::core::TokenStore;
use sha2::{Digest, Sha256};
use std::{fs::OpenOptions, path::PathBuf};

pub struct FileTokenStore {
    directory: PathBuf,
}
impl FileTokenStore {
    pub fn new(directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
        Ok(Self { directory })
    }
    fn path(&self, issuer: &str, jti: &str) -> PathBuf {
        let mut hash = Sha256::new();
        hash.update((issuer.len() as u64).to_be_bytes());
        hash.update(issuer.as_bytes());
        hash.update(jti.as_bytes());
        self.directory
            .join(format!("{:x}.revoked", hash.finalize()))
    }
    async fn insert(&self, issuer: &str, jti: &str, expires_at: i64) -> Result<bool> {
        let path = self.path(issuer, jti);
        let directory = self.directory.clone();
        tokio::task::spawn_blocking(move || {
            if expires_at <= chrono::Utc::now().timestamp() {
                return Ok(false);
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let inserted = match options.open(path) {
                Ok(file) => {
                    file.sync_all()?;
                    true
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(e) => return Err(e.into()),
            };
            // A zero-byte marker is the complete record: cancellation cannot expose a partial token.
            std::fs::File::open(directory)?.sync_all()?;
            Ok(inserted)
        })
        .await?
    }
}
#[async_trait::async_trait]
impl TokenStore for FileTokenStore {
    async fn is_revoked(&self, issuer: &str, jti: &str) -> Result<bool> {
        let path = self.path(issuer, jti);
        match tokio::fs::metadata(path).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    async fn revoke(&self, issuer: &str, jti: &str, expires_at: i64) -> Result<()> {
        self.insert(issuer, jti, expires_at).await?;
        Ok(())
    }
    async fn consume_refresh(&self, issuer: &str, jti: &str, expires_at: i64) -> Result<bool> {
        self.insert(issuer, jti, expires_at).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn refresh_is_single_use_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let a = FileTokenStore::new(dir.path()).unwrap();
        let b = FileTokenStore::new(dir.path()).unwrap();
        let expires = chrono::Utc::now().timestamp() + 60;
        let (x, y) = tokio::join!(
            a.consume_refresh("issuer", "token", expires),
            b.consume_refresh("issuer", "token", expires)
        );
        assert_ne!(x.unwrap(), y.unwrap());
        assert!(FileTokenStore::new(dir.path())
            .unwrap()
            .is_revoked("issuer", "token")
            .await
            .unwrap());
        assert!(!a.is_revoked("another", "token").await.unwrap());
        assert!(!a.consume_refresh("issuer", "expired", 0).await.unwrap());
    }
}
