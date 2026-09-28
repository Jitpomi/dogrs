#![doc = include_str!("../README.md")]

pub mod adapter;
mod bounded;
mod config;
mod coordinator;
mod error;
mod native;
mod receipt;
mod recovery;
mod resources;
pub use bounded::ValidatedUpload;
pub use native::{reconcile_write, NativeMultipartStore, NativePart, RecoveryReport, WriteOutcome};
pub use recovery::{
    FileUploadJournal, MemoryUploadJournal, PendingWrite, UploadJournal, UploadLease,
};
pub use resources::{UploadLimits, UploadResources, UploadUsage};
#[cfg(feature = "s3")]
mod s3_store;
mod session_store;
pub mod store;
mod types;
mod upload;

// Re-export main types for clean API
pub use adapter::{BlobAdapter, BlobState};
pub use config::{BlobConfig, UploadRules};
pub use coordinator::DefaultUploadCoordinator;
pub use error::{BlobError, BlobResult};
pub use receipt::{BlobReceipt, OpenedBlob, OpenedContent, ResolvedRange, UploadInfo};
#[cfg(feature = "s3")]
pub use s3_store::{S3CompatibleStore, S3Config};
pub use session_store::MemoryUploadSessionStore;
pub use store::{
    BlobInfo, BlobKeyStrategy, BlobMetadata, BlobStore, DefaultKeyStrategy, GetResult,
    MultipartBlobStore, ObjectHead, PutResult, SignedUrlBlobStore, StoreCapabilities,
};
pub use types::{
    BlobCtx, BlobId, BlobPut, ByteRange, ByteStream, ChunkResult, ChunkSession, ChunkSessionId,
    PartReceipt, UploadId, UploadProgress, UploadSession, UploadStatus,
};
pub use upload::{UploadCoordinator, UploadIntent, UploadSessionStore};

/// Prelude for convenient imports
pub mod prelude {
    pub use crate::{
        BlobAdapter, BlobConfig, BlobCtx, BlobError, BlobId, BlobPut, BlobReceipt, BlobResult,
        BlobStore, ByteStream,
    };
}
