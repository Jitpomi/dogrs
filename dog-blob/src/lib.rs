//! # dog-blob: Production-ready blob storage infrastructure
//! 
//! `dog-blob` provides streaming-first, resumable, range-friendly blob storage for DogRS applications
//! with zero boilerplate. It's designed to eliminate all the routine media handling code that 
//! services shouldn't have to write.
//! 
//! ## Key Features
//! 
//! - **Streaming-first**: Handle huge video files without buffering entire content in memory
//! - **Multipart/resumable uploads**: Built-in coordination for large files with automatic fallback
//! - **Range requests**: First-class support for video/audio scrubbing and partial content delivery
//! - **Storage agnostic**: Works with any backend (S3, filesystem, memory, custom implementations)
//! - **Server agnostic**: No HTTP coupling - works with any protocol (HTTP, gRPC, CLI, background jobs)
//! - **Zero boilerplate**: Services focus on business logic, not media mechanics
//! 
//! ## Quick Start
//! 
//! ```rust
//! use dog_blob::prelude::*;
//! use tokio_util::io::ReaderStream;
//! use std::io::Cursor;
//! 
//! # #[tokio::main]
//! # async fn main() -> BlobResult<()> {
//! // 1. Create adapter with S3-compatible storage
//! let store = dog_blob::S3CompatibleStore::from_env()?;
//! let adapter = BlobAdapter::new(store, BlobConfig::default());
//! 
//! // 2. Create context for your tenant/user
//! let ctx = BlobCtx::new("my-app".to_string())
//!     .with_actor("user-123".to_string());
//! 
//! // 3. Upload a file
//! let data = b"Hello, world!";
//! let stream = ReaderStream::new(Cursor::new(data));
//! let put_request = BlobPut::new()
//!     .with_content_type("text/plain")
//!     .with_filename("hello.txt");
//! 
//! let receipt = adapter.put(ctx.clone(), put_request, Box::pin(stream)).await?;
//! 
//! // 4. Download with range support
//! let opened = adapter.open(ctx, receipt.id, None).await?;
//! # Ok(())
//! # }
//! ```
//! 
//! ## Architecture
//! 
//! dog-blob follows a clean separation of concerns:
//! 
//! ```text
//! ┌─────────────────┐
//! │   Your Service  │  ← Business logic only
//! ├─────────────────┤
//! │   BlobAdapter   │  ← Media coordination
//! ├─────────────────┤
//! │   BlobStore     │  ← Storage primitives
//! └─────────────────┘
//! ```
//! 
//! The key insight: **BlobAdapter is infrastructure, not a service**. You embed it in your services:
//! 
//! ```rust
//! use dog_blob::prelude::*;
//! 
//! pub struct MediaService {
//!     blobs: BlobAdapter,  // This is all you need!
//! }
//! 
//! impl MediaService {
//!     pub async fn upload_photo(&self, tenant_id: String, data: Vec<u8>) -> BlobResult<String> {
//!         let ctx = BlobCtx::new(tenant_id);
//!         let stream = futures::stream::once(async { Ok(bytes::Bytes::from(data)) });
//!         
//!         // One line handles all the blob complexity
//!         let receipt = self.blobs.put(ctx, BlobPut::new(), Box::pin(stream)).await?;
//!         
//!         Ok(receipt.id.to_string())
//!     }
//! }
//! ```

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
