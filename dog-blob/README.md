# dog-blob

**Media/blob handling infrastructure for DogRS - server and storage agnostic**

dog-blob provides streaming-first, resumable, range-friendly blob storage for DogRS applications with zero boilerplate. It's designed to eliminate all the routine media handling code that services shouldn't have to write.

## Features

- **Streaming-first**: Handle huge video files without buffering
- **Multipart/resumable uploads**: Built-in coordination for large files
- **Range requests**: First-class support for video/audio scrubbing
- **Storage agnostic**: Works with any backend (memory, filesystem, S3, etc.)
- **Server agnostic**: No HTTP coupling - works with any protocol
- **Zero boilerplate**: Services focus on business logic, not media mechanics

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
dog-blob = { path = "../dog-blob", features = ["memory"] }
```

### Basic Usage

```rust
use dog_blob::{BlobAdapter, BlobConfig, BlobCtx, BlobPut};
use dog_blob::memory::MemoryStore;

// Create adapter with memory store
let store = MemoryStore::new();
let config = BlobConfig::default();
let adapter = BlobAdapter::new(store, config);

// Create context (tenant + user info)
let ctx = BlobCtx::new("tenant1".to_string())
    .with_actor("user123".to_string());

// Store a blob
let put_request = BlobPut::new()
    .with_content_type("image/jpeg")
    .with_filename("photo.jpg");

let receipt = adapter.put(ctx.clone(), put_request, stream).await?;

// Retrieve with range support (for video scrubbing)
let opened = adapter.open(ctx, receipt.id, Some(ByteRange::new(0, Some(1024)))).await?;
```

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

### Memory Store (for testing)

```rust
use dog_blob::memory::MemoryStore;

let store = MemoryStore::new();
let adapter = BlobAdapter::new(store, BlobConfig::default());
```

### Filesystem Store (coming soon)

```rust
use dog_blob::fs::FsStore;

let store = FsStore::new("./uploads")?;
let adapter = BlobAdapter::new(store, BlobConfig::default());
```

### S3-Compatible Store (coming soon)

```rust
use dog_blob::s3::S3Store;

let store = S3Store::from_env()?;
let adapter = BlobAdapter::new(store, BlobConfig::default());
```

## Multipart/Resumable Uploads

dog-blob handles multipart uploads automatically:

```rust
// Begin multipart upload
let session = adapter.begin_multipart(ctx.clone(), put_request).await?;

// Upload parts (can be done in parallel, out of order)
let part1 = adapter.upload_part(ctx.clone(), session.upload_id, 1, stream1).await?;
let part2 = adapter.upload_part(ctx.clone(), session.upload_id, 2, stream2).await?;

// Complete upload
let receipt = adapter.complete_multipart(ctx, session.upload_id).await?;
```

The coordinator handles:
- Native multipart (when storage supports it)
- Staged assembly (universal fallback)
- Part validation and ordering
- Cleanup on abort/failure

## Range Requests (Video Streaming)

Perfect for video/audio scrubbing:

```rust
// Request specific byte range
let range = ByteRange::new(1024, Some(2048));
let opened = adapter.open(ctx, blob_id, Some(range)).await?;

match opened.content {
    OpenedContent::Stream { stream, resolved_range } => {
        // Stream the partial content
        // HTTP servers can return 206 Partial Content
    }
    OpenedContent::SignedUrl { url, expires_at } => {
        // Redirect to signed URL (if storage supports it)
    }
}
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

## Examples

See `examples/` directory:

- `basic_usage.rs` - Simple put/get/delete operations
- `media_service.rs` - Integration with a service layer

## Roadmap

- [ ] Filesystem store implementation
- [ ] S3-compatible store implementation  
- [ ] Video processing pipeline (optional)
- [ ] Signed URL support
- [ ] Compression support
- [ ] Deduplication support

## License

MIT OR Apache-2.0
