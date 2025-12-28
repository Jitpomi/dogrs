//! dog-blob: Media/blob handling infrastructure for DogRS
//! 
//! Provides server-agnostic, storage-agnostic blob handling with:
//! - Streaming uploads/downloads
//! - Multipart/resumable uploads
//! - Range requests (video/audio scrubbing)
//! - Pluggable storage backends
//! - Zero boilerplate for DogService implementations

pub mod adapter;
pub mod config;
pub mod coordinator;
pub mod error;
pub mod receipt;
pub mod store;
pub mod types;
pub mod upload;



// Re-export main types for clean API
pub use adapter::BlobAdapter;
pub use config::{BlobConfig, UploadRules};
pub use coordinator::DefaultUploadCoordinator;
pub use error::{BlobError, BlobResult};
pub use receipt::{BlobReceipt, OpenedBlob, ResolvedRange};
pub use store::{
    BlobStore, MultipartBlobStore, SignedUrlBlobStore, BlobKeyStrategy, DefaultKeyStrategy,
    PutResult, GetResult, ObjectHead, StoreCapabilities
};
pub use types::{
    BlobCtx, BlobId, BlobPut, ByteRange, ByteStream, 
    UploadId, UploadSession, UploadStatus, PartReceipt, UploadProgress
};
pub use upload::{UploadCoordinator, UploadIntent, UploadSessionStore};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{
        BlobAdapter, BlobConfig, BlobError, BlobResult, BlobReceipt, 
        BlobStore, BlobCtx, BlobId, BlobPut, ByteStream
    };
}
