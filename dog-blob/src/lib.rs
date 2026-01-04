//! dog-blob: Media/blob handling infrastructure for DogRS
//! 
//! Provides server-agnostic, storage-agnostic blob handling with:
//! - Streaming uploads/downloads
//! - Multipart/resumable uploads
//! - Range requests (video/audio scrubbing)
//! - Pluggable storage backends
//! - Zero boilerplate for DogService implementations

pub mod adapter;
mod config;
mod coordinator;
mod error;
mod receipt;
mod s3_store;
mod session_store;
pub mod store;
mod types;
mod upload;



// Re-export main types for clean API
pub use adapter::BlobAdapter;
pub use config::{BlobConfig, UploadRules};
pub use coordinator::DefaultUploadCoordinator;
pub use error::{BlobError, BlobResult};
pub use receipt::{BlobReceipt, OpenedBlob, ResolvedRange};
pub use s3_store::{S3CompatibleStore, S3Config};
pub use store::{
    BlobInfo, BlobMetadata, BlobStore, MultipartBlobStore, SignedUrlBlobStore, BlobKeyStrategy, DefaultKeyStrategy,
    PutResult, GetResult, ObjectHead, StoreCapabilities
};
pub use types::{
    BlobCtx, BlobId, BlobPut, ByteRange, ByteStream, 
    UploadId, UploadSession, UploadStatus, PartReceipt, UploadProgress,
    ChunkSessionId, ChunkResult, ChunkSession
};
pub use upload::{UploadCoordinator, UploadIntent, UploadSessionStore};
pub use session_store::MemoryUploadSessionStore;

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{
        BlobAdapter, BlobConfig, BlobError, BlobResult, BlobReceipt, 
        BlobStore, BlobCtx, BlobId, BlobPut, ByteStream
    };
}
