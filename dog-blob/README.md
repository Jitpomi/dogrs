# dog-blob

**Production-ready blob storage infrastructure for DogRS applications**

[![Crates.io](https://img.shields.io/crates/v/dog-blob.svg)](https://crates.io/crates/dog-blob)
[![Documentation](https://docs.rs/dog-blob/badge.svg)](https://docs.rs/dog-blob)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

dog-blob provides streaming-first, resumable, range-friendly blob storage for DogRS applications with zero boilerplate. It's designed to eliminate all the routine media handling code that services shouldn't have to write.

**Perfect for:** Video streaming, file uploads, media processing, document storage, backup systems, and any application that needs reliable blob handling.

## Features

- **Streaming-first**: Handle huge video files without buffering
- **Multipart/resumable uploads**: Built-in coordination for large files
- **Range requests**: First-class support for video/audio scrubbing
- **Storage agnostic**: Works with any backend (memory, filesystem, S3, etc.)
- **Server agnostic**: No HTTP coupling - works with any protocol
- **Zero boilerplate**: Services focus on business logic, not media mechanics

## Table of Contents

- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
- [Storage Backends](#storage-backends)
- [Advanced Features](#advanced-features)
- [Real-World Examples](#real-world-examples)
- [API Reference](#api-reference)
- [Performance & Best Practices](#performance--best-practices)

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-blob = "0.1.0"
```

### 30-Second Example

```rust
use dog_blob::prelude::*;
use tokio_util::io::ReaderStream;
use std::io::Cursor;

#[tokio::main]
async fn main() -> BlobResult<()> {
    // 1. Create adapter (using S3-compatible storage)
    let store = dog_blob::S3CompatibleStore::from_env()?;
    let adapter = BlobAdapter::new(store, BlobConfig::default());

    // 2. Create context for your tenant/user
    let ctx = BlobCtx::new("my-app".to_string())
        .with_actor("user-123".to_string());

    // 3. Upload a file
    let data = b"Hello, world!";
    let stream = ReaderStream::new(Cursor::new(data));
    let put_request = BlobPut::new()
        .with_content_type("text/plain")
        .with_filename("hello.txt");

    let receipt = adapter.put(ctx.clone(), put_request, Box::pin(stream)).await?;
    println!("Uploaded! ID: {}", receipt.id);

    // 4. Download with range support
    let opened = adapter.open(ctx, receipt.id, None).await?;
    println!("Downloaded {} bytes", opened.content_length());

    Ok(())
}
```

### Core Concepts

dog-blob is built around four key concepts:

1. **`BlobAdapter`** - The main interface you embed in your services
2. **`BlobStore`** - Pluggable storage backend (S3, filesystem, memory, etc.)
3. **`BlobCtx`** - Tenant/user context for multi-tenant applications
4. **`BlobReceipt`** - Portable metadata returned after successful storage

### Service Integration

The key insight: **BlobAdapter is infrastructure, not a service**. You embed it in your services:

```rust
pub struct MediaService {
    blobs: BlobAdapter,  // This is all you need!
}

impl MediaService {
    pub async fn upload_photo(&self, tenant_id: String, data: Vec<u8>) -> Result<MediaResponse, Error> {
        let ctx = BlobCtx::new(tenant_id);
        let stream = stream::once(async { Ok(Bytes::from(data)) });
        
        // One line handles all the blob complexity
        let receipt = self.blobs.put(ctx, BlobPut::new(), Box::pin(stream)).await?;
        
        Ok(MediaResponse {
            id: receipt.id.to_string(),
            url: format!("/media/{}", receipt.id),
            size: receipt.size_bytes,
        })
    }
}
```

## Architecture

dog-blob follows a clean separation of concerns:

```
┌─────────────────┐
│   Your Service  │  ← Business logic only
├─────────────────┤
│   BlobAdapter   │  ← Media coordination
├─────────────────┤
│   BlobStore     │  ← Storage primitives
└─────────────────┘
```

### Core Types

- **`BlobAdapter`**: Main interface - embed this in your services
- **`BlobStore`**: Storage backend trait (implement for new storage types)
- **`BlobReceipt`**: Portable metadata returned after storage
- **`BlobCtx`**: Tenant/user context for multi-tenant apps

## Storage Backends

dog-blob supports multiple storage backends through the `BlobStore` trait:

### S3-Compatible Store (Production Ready)

Works with AWS S3, MinIO, DigitalOcean Spaces, and other S3-compatible services:

```rust
use dog_blob::{S3CompatibleStore, S3Config, BlobAdapter, BlobConfig};

// From environment variables (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, etc.)
let store = S3CompatibleStore::from_env()?;

// Or configure manually
let config = S3Config::new("my-bucket")
    .with_region("us-west-2")
    .with_endpoint("https://s3.amazonaws.com")
    .with_credentials("access_key", "secret_key");

let store = S3CompatibleStore::new(config)?;
let adapter = BlobAdapter::new(store, BlobConfig::default());
```

**Features:**
- ✅ Native multipart uploads
- ✅ Signed URL generation
- ✅ Range request support
- ✅ Server-side encryption
- ✅ Cross-region replication

### Memory Store (Testing & Development)

Perfect for unit tests and development:

```rust
use dog_blob::store::MemoryBlobStore;

let store = MemoryBlobStore::new();
let adapter = BlobAdapter::new(store, BlobConfig::default());
```

**Features:**
- ✅ Zero configuration
- ✅ Fast in-memory operations
- ✅ Automatic cleanup
- ❌ Not persistent (data lost on restart)

### Custom Storage Implementation

Implement `BlobStore` for your own storage backend:

```rust
use dog_blob::{BlobStore, BlobResult, BlobCtx, BlobId, ByteStream};
use async_trait::async_trait;

pub struct MyCustomStore {
    // Your storage implementation
}

#[async_trait]
impl BlobStore for MyCustomStore {
    async fn put(&self, ctx: BlobCtx, id: BlobId, stream: ByteStream) -> BlobResult<PutResult> {
        // Implement storage logic
        todo!()
    }

    async fn get(&self, ctx: BlobCtx, id: BlobId, range: Option<ByteRange>) -> BlobResult<GetResult> {
        // Implement retrieval logic
        todo!()
    }

    // ... implement other required methods
}
```

## Advanced Features

### Multipart/Resumable Uploads

dog-blob handles multipart uploads automatically for large files:

```rust
use dog_blob::prelude::*;

// Configure multipart thresholds
let config = BlobConfig::new()
    .with_multipart_threshold(16 * 1024 * 1024)  // 16MB threshold
    .with_upload_rules(
        UploadRules::new()
            .with_part_size(8 * 1024 * 1024)     // 8MB parts
            .with_max_parts(10_000)              // Max 10k parts
    );

let adapter = BlobAdapter::new(store, config);

// For files > 16MB, this automatically becomes multipart
let large_file = std::fs::File::open("video.mp4")?;
let stream = ReaderStream::new(large_file);
let put_request = BlobPut::new()
    .with_content_type("video/mp4")
    .with_filename("my-video.mp4");

// Automatically handles multipart coordination
let receipt = adapter.put(ctx, put_request, Box::pin(stream)).await?;
```

**Manual multipart control:**

```rust
// Begin multipart upload
let session = adapter.begin_multipart(ctx.clone(), put_request).await?;

// Upload parts (can be done in parallel, out of order)
let part1 = adapter.upload_part(ctx.clone(), session.upload_id, 1, stream1).await?;
let part2 = adapter.upload_part(ctx.clone(), session.upload_id, 2, stream2).await?;

// Complete upload
let receipt = adapter.complete_multipart(ctx, session.upload_id).await?;
```

**The coordinator automatically handles:**
- ✅ Native multipart (when storage supports it)
- ✅ Staged assembly (universal fallback)
- ✅ Part validation and ordering
- ✅ Cleanup on abort/failure
- ✅ Resume from partial uploads

### Range Requests (Video Streaming)

Perfect for video/audio scrubbing and progressive downloads:

```rust
use dog_blob::{ByteRange, OpenedContent};

// Request specific byte range (bytes 1024-2048)
let range = ByteRange::new(1024, Some(2048));
let opened = adapter.open(ctx, blob_id, Some(range)).await?;

match opened.content {
    OpenedContent::Stream { stream, resolved_range } => {
        // Stream the partial content
        // HTTP servers can return 206 Partial Content
        println!("Streaming bytes {}-{}", 
                resolved_range.start, 
                resolved_range.end.unwrap_or(opened.content_length()));
        
        // Process the stream
        while let Some(chunk) = stream.next().await {
            let bytes = chunk?;
            // Handle chunk...
        }
    }
    OpenedContent::SignedUrl { url, expires_at } => {
        // Redirect to signed URL (if storage supports it)
        println!("Redirect to: {} (expires: {:?})", url, expires_at);
    }
}
```

**Common use cases:**
- 🎥 Video seeking/scrubbing
- 🎵 Audio preview generation
- 📄 PDF page extraction
- 🖼️ Image thumbnail generation
- 📊 Large file sampling

### Metadata and Custom Fields

Store rich metadata alongside your blobs:

```rust
use serde_json::json;

let put_request = BlobPut::new()
    .with_content_type("video/mp4")
    .with_filename("presentation.mp4")
    .with_metadata(json!({
        "title": "Q4 Sales Presentation",
        "duration_seconds": 1800,
        "resolution": "1920x1080",
        "tags": ["sales", "quarterly", "presentation"],
        "created_by": "user-123",
        "department": "sales"
    }));

let receipt = adapter.put(ctx, put_request, stream).await?;

// Retrieve metadata later
let info = adapter.info(ctx, receipt.id).await?;
println!("Video title: {}", info.metadata.custom["title"]);
```

## Configuration

```rust
let config = BlobConfig::new()
    .with_max_blob_bytes(5 * 1024 * 1024 * 1024) // 5GB limit
    .with_multipart_threshold(16 * 1024 * 1024)   // 16MB threshold
    .with_upload_rules(
        UploadRules::new()
            .with_part_size(8 * 1024 * 1024)      // 8MB parts
            .with_max_parts(10_000)               // Max 10k parts
    );
```

## Comparison to feathers-blob

| Feature | feathers-blob | dog-blob |
|---------|---------------|----------|
| Input model | Buffer/data-URI | Streaming ByteStream |
| Large files | Manual | First-class |
| Multipart | External middleware | Built-in coordination |
| Resumable | Not native | Native |
| Range reads | App-implemented | First-class API |
| Storage abstraction | abstract-blob-store | Pluggable traits |
| Metadata storage | Service-specific | Portable receipts |
| Server coupling | HTTP-centric | Server-agnostic |

## Design Philosophy

1. **Media is infrastructure, not a service** - BlobAdapter is a capability you embed
2. **Streaming everywhere** - Never buffer entire files in memory
3. **Storage agnostic** - Switch backends without changing service code
4. **Server agnostic** - Works with HTTP, gRPC, CLI, background jobs
5. **Zero opinions** - You decide metadata schema, ACL, business logic

## Real-World Examples

### Video Streaming Service

```rust
use dog_blob::prelude::*;
use serde_json::json;

pub struct VideoService {
    blobs: BlobAdapter,
}

impl VideoService {
    pub async fn upload_video(&self, user_id: String, video_stream: ByteStream) -> BlobResult<String> {
        let ctx = BlobCtx::new("video-app".to_string())
            .with_actor(user_id.clone());

        let put_request = BlobPut::new()
            .with_content_type("video/mp4")
            .with_metadata(json!({
                "uploaded_by": user_id,
                "upload_time": chrono::Utc::now(),
                "processing_status": "pending"
            }));

        let receipt = self.blobs.put(ctx, put_request, video_stream).await?;
        Ok(receipt.id.to_string())
    }

    pub async fn stream_video_segment(&self, video_id: String, start_byte: u64, end_byte: Option<u64>) -> BlobResult<ByteStream> {
        let ctx = BlobCtx::new("video-app".to_string());
        let blob_id = BlobId(video_id);
        let range = ByteRange::new(start_byte, end_byte);

        let opened = self.blobs.open(ctx, blob_id, Some(range)).await?;
        
        match opened.content {
            OpenedContent::Stream { stream, .. } => Ok(stream),
            OpenedContent::SignedUrl { url, .. } => {
                // Redirect client to signed URL for direct streaming
                Err(BlobError::RedirectToUrl(url))
            }
        }
    }
}
```

### Document Management System

```rust
pub struct DocumentService {
    blobs: BlobAdapter,
}

impl DocumentService {
    pub async fn store_document(&self, tenant_id: String, doc: Document) -> BlobResult<DocumentReceipt> {
        let ctx = BlobCtx::new(tenant_id.clone())
            .with_actor(doc.created_by.clone());

        let put_request = BlobPut::new()
            .with_content_type(&doc.mime_type)
            .with_filename(&doc.filename)
            .with_metadata(json!({
                "document_type": doc.doc_type,
                "tags": doc.tags,
                "department": doc.department,
                "confidentiality": doc.confidentiality_level,
                "retention_policy": doc.retention_days
            }));

        let receipt = self.blobs.put(ctx, put_request, doc.content_stream).await?;
        
        Ok(DocumentReceipt {
            id: receipt.id.to_string(),
            size: receipt.size_bytes,
            checksum: receipt.etag,
            storage_class: receipt.storage_class,
        })
    }

    pub async fn generate_preview(&self, doc_id: String, page: u32) -> BlobResult<ByteStream> {
        // Get first 64KB for preview generation
        let preview_range = ByteRange::new(0, Some(64 * 1024));
        let ctx = BlobCtx::new("doc-service".to_string());
        
        let opened = self.blobs.open(ctx, BlobId(doc_id), Some(preview_range)).await?;
        
        match opened.content {
            OpenedContent::Stream { stream, .. } => {
                // Process stream to generate preview
                Ok(stream)
            }
            _ => Err(BlobError::UnsupportedOperation("Preview generation requires streaming".into()))
        }
    }
}
```

### Backup Service with Chunked Uploads

```rust
pub struct BackupService {
    blobs: BlobAdapter,
}

impl BackupService {
    pub async fn backup_large_database(&self, db_path: &str) -> BlobResult<String> {
        let ctx = BlobCtx::new("backup-service".to_string());
        
        // Configure for large files
        let config = BlobConfig::new()
            .with_multipart_threshold(100 * 1024 * 1024)  // 100MB threshold
            .with_upload_rules(
                UploadRules::new()
                    .with_part_size(50 * 1024 * 1024)     // 50MB parts
                    .with_max_parts(2000)                 // Support up to 100GB files
            );

        let put_request = BlobPut::new()
            .with_content_type("application/octet-stream")
            .with_filename(&format!("backup-{}.sql", chrono::Utc::now().format("%Y%m%d")))
            .with_metadata(json!({
                "backup_type": "full",
                "database": "production",
                "compression": "gzip",
                "encrypted": true
            }));

        // Stream large file directly from disk
        let file = tokio::fs::File::open(db_path).await?;
        let stream = ReaderStream::new(file);

        let receipt = self.blobs.put(ctx, put_request, Box::pin(stream)).await?;
        Ok(receipt.id.to_string())
    }
}
```

## API Reference

### Core Types

#### `BlobAdapter`
The main interface for blob operations.

```rust
impl BlobAdapter {
    // Basic operations
    pub async fn put(&self, ctx: BlobCtx, request: BlobPut, stream: ByteStream) -> BlobResult<BlobReceipt>;
    pub async fn open(&self, ctx: BlobCtx, id: BlobId, range: Option<ByteRange>) -> BlobResult<OpenedBlob>;
    pub async fn info(&self, ctx: BlobCtx, id: BlobId) -> BlobResult<BlobInfo>;
    pub async fn delete(&self, ctx: BlobCtx, id: BlobId) -> BlobResult<()>;

    // Multipart operations
    pub async fn begin_multipart(&self, ctx: BlobCtx, request: BlobPut) -> BlobResult<UploadSession>;
    pub async fn upload_part(&self, ctx: BlobCtx, upload_id: UploadId, part_number: u32, stream: ByteStream) -> BlobResult<PartReceipt>;
    pub async fn complete_multipart(&self, ctx: BlobCtx, upload_id: UploadId) -> BlobResult<BlobReceipt>;
    pub async fn abort_multipart(&self, ctx: BlobCtx, upload_id: UploadId) -> BlobResult<()>;
}
```

#### `BlobCtx`
Context for multi-tenant operations.

```rust
impl BlobCtx {
    pub fn new(tenant_id: String) -> Self;
    pub fn with_actor(self, actor_id: String) -> Self;
    pub fn with_trace_id(self, trace_id: String) -> Self;
}
```

#### `BlobPut`
Request builder for blob uploads.

```rust
impl BlobPut {
    pub fn new() -> Self;
    pub fn with_content_type(self, content_type: &str) -> Self;
    pub fn with_filename(self, filename: &str) -> Self;
    pub fn with_metadata(self, metadata: serde_json::Value) -> Self;
    pub fn with_storage_class(self, class: &str) -> Self;
}
```

## Performance & Best Practices

### Streaming Best Practices

1. **Always stream large files** - Never load entire files into memory
2. **Use appropriate buffer sizes** - 8MB chunks work well for most cases
3. **Enable multipart for large files** - Set threshold around 16-100MB
4. **Implement backpressure** - Don't overwhelm slower storage backends

```rust
// ✅ Good: Streaming from disk
let file = tokio::fs::File::open("large-video.mp4").await?;
let stream = ReaderStream::new(file);
let receipt = adapter.put(ctx, request, Box::pin(stream)).await?;

// ❌ Bad: Loading entire file into memory
let data = tokio::fs::read("large-video.mp4").await?;
let stream = stream::once(async { Ok(Bytes::from(data)) });
let receipt = adapter.put(ctx, request, Box::pin(stream)).await?;
```

### Configuration Tuning

```rust
// For high-throughput video streaming
let config = BlobConfig::new()
    .with_multipart_threshold(50 * 1024 * 1024)   // 50MB
    .with_upload_rules(
        UploadRules::new()
            .with_part_size(25 * 1024 * 1024)     // 25MB parts
            .with_max_parts(4000)                 // Up to 100GB files
    );

// For document storage (smaller files)
let config = BlobConfig::new()
    .with_multipart_threshold(10 * 1024 * 1024)   // 10MB
    .with_upload_rules(
        UploadRules::new()
            .with_part_size(5 * 1024 * 1024)      // 5MB parts
            .with_max_parts(1000)                 // Up to 5GB files
    );
```

### Error Handling

```rust
use dog_blob::{BlobError, BlobResult};

match adapter.put(ctx, request, stream).await {
    Ok(receipt) => {
        println!("Upload successful: {}", receipt.id);
    }
    Err(BlobError::StorageQuotaExceeded) => {
        // Handle quota exceeded
        return Err("Storage quota exceeded".into());
    }
    Err(BlobError::InvalidContentType(ct)) => {
        // Handle invalid content type
        return Err(format!("Invalid content type: {}", ct).into());
    }
    Err(BlobError::NetworkError(e)) => {
        // Retry logic for network errors
        tokio::time::sleep(Duration::from_secs(1)).await;
        // Retry upload...
    }
    Err(e) => {
        // Handle other errors
        eprintln!("Upload failed: {}", e);
    }
}
```

### Monitoring and Observability

```rust
// Enable tracing for observability
use tracing::{info, error, instrument};

impl VideoService {
    #[instrument(skip(self, video_stream))]
    pub async fn upload_video(&self, user_id: String, video_stream: ByteStream) -> BlobResult<String> {
        info!("Starting video upload for user: {}", user_id);
        
        let start = std::time::Instant::now();
        let result = self.blobs.put(ctx, request, video_stream).await;
        
        match &result {
            Ok(receipt) => {
                info!(
                    "Video upload completed in {:?}, size: {} bytes, id: {}", 
                    start.elapsed(), 
                    receipt.size_bytes, 
                    receipt.id
                );
            }
            Err(e) => {
                error!("Video upload failed after {:?}: {}", start.elapsed(), e);
            }
        }
        
        result.map(|r| r.id.to_string())
    }
}
```

## Examples

The `dog-examples/` directory contains real-world implementations:

- **`music-blobs/`** - Complete music streaming application with waveform visualization
  - Demonstrates S3-compatible storage integration
  - Shows multipart upload handling for large audio files
  - Implements range requests for audio scrubbing
  - Features real-time waveform generation using Symphonia audio decoding
  - Production-ready service architecture with dog-blob integration

## Roadmap

- [x] S3-compatible store implementation ✅
- [x] Multipart upload coordination ✅
- [x] Range request support ✅
- [x] Production examples (music-blobs) ✅
- [ ] Filesystem store implementation
- [ ] Video processing pipeline (optional)
- [ ] Signed URL support
- [ ] Compression support
- [ ] Deduplication support

## License

MIT OR Apache-2.0
