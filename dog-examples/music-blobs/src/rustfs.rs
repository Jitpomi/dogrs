use crate::rustfs_store::RustFSStore;
use crate::services::MusicParams;
use crate::upload_store::MemoryUploadSessionStore;
use anyhow::Result;
use dog_blob::adapter::BlobState;
use dog_blob::{BlobConfig, DefaultKeyStrategy, DefaultUploadCoordinator};
use dog_core::DogApp;
use serde_json::Value;
use std::sync::Arc;

/// RustFsState contains a BlobState following RustFS documentation pattern
pub struct RustFsState {
    pub blob_state: Arc<BlobState>,
}

impl RustFsState {
    pub async fn setup_store(app: &DogApp<Value, MusicParams>) -> Result<()> {
        // Create RustFS storage with production credentials
        let bucket = std::env::var("RUSTFS_BUCKET").unwrap_or_else(|_| "music-blobs".to_string());

        let storage = RustFSStore::new(bucket).await?;

        // Configure blob handling
        let config = BlobConfig {
            max_blob_bytes: 100_000_000,          // 100MB max
            multipart_threshold_bytes: 5_000_000, // 5MB threshold - trigger multipart for music files
            upload_rules: dog_blob::UploadRules {
                part_size: 5_000_000, // 5MB parts
                max_parts: 100,
                require_fixed_part_size: true,
                allow_out_of_order: true,
            },
            require_range_support: false,
            checksum_alg: None,
        };

        println!("🔧 Dog-blob configuration:");
        println!("   Max blob size: {} MB", config.max_blob_bytes / 1_000_000);
        println!(
            "   Multipart threshold: {} MB",
            config.multipart_threshold_bytes / 1_000_000
        );
        println!(
            "   Part size: {} MB",
            config.upload_rules.part_size / 1_000_000
        );

        // Create upload coordinator with session store
        let session_store = MemoryUploadSessionStore::new();
        let coordinator = DefaultUploadCoordinator::new(
            storage.clone(),
            session_store,
            DefaultKeyStrategy,
            config.clone(),
        );

        // Create BlobState and then RustFsState containing it
        let blob_state = Arc::new(BlobState::new(storage, config).with_uploads(coordinator));

        let state = Arc::new(RustFsState { blob_state });
        app.set("rustfs", state);

        Ok(())
    }
}
