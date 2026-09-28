use crate::rustfs_store::RustFSStore;
use crate::services::MusicParams;
use anyhow::Result;
use dog_blob::adapter::BlobState;
use dog_blob::MemoryUploadSessionStore;
use dog_blob::{BlobConfig, DefaultKeyStrategy, DefaultUploadCoordinator};
use serde_json::Value;
use std::sync::Arc;

/// RustFsState contains a BlobState following RustFS documentation pattern
pub struct RustFsState {
    pub blob_state: Arc<BlobState>,
    pub rustfs_store: RustFSStore,
    pub receipts_directory: std::path::PathBuf,
}

impl RustFsState {
    pub async fn setup_store(app: &mut dog_core::DogAppBuilder<Value, MusicParams>) -> Result<()> {
        // Create RustFS storage with production credentials
        let bucket = std::env::var("RUSTFS_BUCKET").unwrap_or_else(|_| "music-blobs".to_string());

        let directory = std::path::PathBuf::from(
            std::env::var("MUSIC_DATA_DIR").unwrap_or_else(|_| ".dogrs-music".into()),
        );
        let mut builder = tokio::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&directory).await?;
        builder.create(directory.join("staging")).await?;
        builder.create(directory.join("receipts")).await?;
        let resources = dog_blob::UploadResources::new(dog_blob::UploadLimits {
            staging_directory: Some(directory.join("staging")),
            max_concurrent_uploads: 4,
            max_staging_bytes: 1024 * 1024 * 1024,
            upload_timeout: std::time::Duration::from_secs(120),
            idle_timeout: std::time::Duration::from_secs(15),
        })?;
        resources.cleanup_staging().await?;
        let storage = RustFSStore::new(bucket)
            .await?
            .with_resources(resources.clone());

        // Configure blob handling
        let config = BlobConfig {
            max_blob_bytes: 100_000_000,          // 100MB max
            multipart_threshold_bytes: 5_000_000, // 5MB threshold - trigger multipart for music files
            upload_rules: dog_blob::UploadRules {
                part_size: 5 * 1024 * 1024, // 5MB parts
                max_parts: 100,
                require_fixed_part_size: true,
                allow_out_of_order: true,
            },
            require_range_support: false,
            checksum_alg: None,
            ..BlobConfig::default()
        };

        // Configuration applied

        // Create upload coordinator with session store
        let session_store = MemoryUploadSessionStore::new();
        let coordinator = DefaultUploadCoordinator::new(
            storage.clone(),
            session_store,
            DefaultKeyStrategy,
            config.clone(),
        )
        .with_resources(resources.clone());

        // Create BlobState and then RustFsState containing it
        let blob_state = Arc::new(
            BlobState::new(storage.clone(), config)
                .with_uploads(coordinator)
                .with_resources(resources)
                .with_journal(Arc::new(dog_blob::FileUploadJournal::new(
                    directory.join("journal"),
                )?)),
        );

        let state = Arc::new(RustFsState {
            blob_state,
            rustfs_store: storage,
            receipts_directory: directory.join("receipts"),
        });
        app.set("rustfs", state);

        Ok(())
    }
}
