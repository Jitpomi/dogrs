# dog-blob

Tenant-scoped object storage with pluggable backends, bounded uploads, range reads,
and resumable multipart coordination. No web server or database is required by the
core crate. The built-in S3-compatible store is enabled with `features = ["s3"]`;
default builds do not depend on the AWS SDK.

## Adapter setup

Supply any implementation of `BlobStore`. This example compiles without S3:

```rust
use dog_blob::{BlobAdapter, BlobConfig, BlobState, BlobStore};
use std::sync::Arc;

fn adapter(store: impl BlobStore + 'static) -> BlobAdapter {
    let config = BlobConfig::default()
        .with_max_blob_bytes(64 * 1024 * 1024)
        .with_checksum("sha256");
    BlobAdapter::new(Arc::new(BlobState::new(store, config)))
}
```

Set context from authenticated server state, not request JSON. Blob reads and
deletes are tenant-scoped; multipart and chunk sessions also require the same
actor (including `None`). Apply your application's per-object authorization before
calling the adapter. `BlobCtx` is trusted context, not an authentication mechanism.

```rust
use dog_blob::{BlobAdapter, BlobCtx, BlobPut, BlobResult, BlobReceipt, ByteStream};

async fn save(adapter: &BlobAdapter, authenticated_tenant: String,
              authenticated_actor: String, body: ByteStream) -> BlobResult<BlobReceipt> {
    let ctx = BlobCtx::new(authenticated_tenant).with_actor(authenticated_actor);
    adapter.put(ctx, BlobPut::new().with_content_type("application/octet-stream"), body).await
}
```

Persist the returned receipt in your application's database. Attributes and the
original filename in receipts are application metadata: `open` cannot reconstruct
all receipt fields from a backend `head` response.

## Limits and file ownership

Uploads are streamed to private temporary files, counting actual bytes before any
backend commit. An optional `size_hint` must equal the actual size. Streams that
exceed `max_blob_bytes` or return an error never reach the final storage write.
Only SHA-256 is supported for configured checksums. Parts are always hashed to
verify assembly integrity. Memory usage scales with stream chunks, not the whole
blob; callers should also bound the chunks their streams produce.

Disk space is required for staging. Adapter, coordinator and S3 staging can coexist,
so provision several times the maximum upload size per concurrent operation.
Apply request deadlines, concurrency limits and disk quotas in your application.
Cancellation releases local temporary files. Backend writes with uncertain outcomes
can leave orphan objects; reconcile them against persisted receipts/session manifests.

`extract_file_data` and `put_from_multipart` accept base64 content with an 8 MiB
decoded limit (chunk requests also obey configured part limits). They reject JSON
`temp_path` objects. For large HTTP uploads, stream the body or pass an already
trusted, open `tokio::fs::File` to `put_file`. The library never deletes that file.
Do not convert a client-supplied path into a trusted file handle.

## Multipart recovery

```rust
use dog_blob::{BlobAdapter, BlobConfig, BlobState, BlobStore, DefaultKeyStrategy,
    DefaultUploadCoordinator, MemoryUploadSessionStore};
use std::sync::Arc;

fn resumable<S: BlobStore + Clone + 'static>(store: S) -> BlobAdapter {
    let config = BlobConfig::default();
    let coordinator = DefaultUploadCoordinator::new(
        store.clone(), MemoryUploadSessionStore::new(), DefaultKeyStrategy, config.clone());
    BlobAdapter::new(Arc::new(BlobState::new(store, config).with_uploads(coordinator)))
}
```

Parts are numbered from 1. The declared final part may be shorter; other parts
must match `part_size` by default. Total counts, byte totals and optional ordering
are enforced. Each accepted part has an immutable server-generated storage key.
Concurrent changes use session revisions; a rejected concurrent write must be
retried after reloading the session.

Completion atomically freezes the manifest as `Completing`, verifies every part's
size and checksum, and writes a stable final key. Retry completion after cancellation
or uncertain I/O. Frozen sessions cannot accept parts or be aborted; retries use the
same bytes. Completed calls return the same blob identity. Staged parts are retained
until the application persists the receipt and calls `forget_upload` (or coordinator
`forget`). Forgetting ends the retry window and removes staging data, not the final blob.

`MemoryUploadSessionStore` is bounded (1024 sessions by default) and **not durable
across process restart**. Durable recovery requires your own `UploadSessionStore`
with atomic compare-and-swap across every process using it. A legacy get/update
implementation is rejected before accepting bytes. Enforce immutable ownership,
keys and the state transitions used by the memory implementation. All coordinators
sharing a session store must use the same keys and upload configuration. Never edit
session records while a coordinator is active.

Sessions expire after an absolute lifetime (one hour by default); active expired
sessions cannot receive parts or begin completion. Periodically inspect expired
sessions, abort active ones with their trusted owner context, then forget them after
cleanup. Retry completion for `Completing` sessions, even after expiry. Terminal
records retain capacity until explicitly forgotten. Superseded or canceled part
writes may leave unreferenced staging objects: a reconciler must compare storage
with session manifests and account for in-flight writes before deleting orphans.

Chunk uploads (`put_chunk`, including Dropzone metadata) are numbered from 0 and
use private temporary directories. They support identical retries, reject changed
metadata/content, and isolate sessions by tenant and actor. They are local to one
adapter/process; use sticky routing or the multipart coordinator for multiple
processes. Call `forget_chunk` after saving a completed receipt, and periodically
call `cleanup_expired_chunks`. Chunk state and files do not survive restart.
`ChunkSession` is a legacy DTO; the adapter does not use it for its private state.

## S3-compatible backend

Enable the `s3` feature, then use `S3CompatibleStore::with_config(bucket, S3Config)`.
`new(bucket)` reads `RUSTFS_REGION`, `RUSTFS_ACCESS_KEY_ID`,
`RUSTFS_SECRET_ACCESS_KEY`, and `RUSTFS_ENDPOINT_URL` for compatibility.
Credentials are redacted from `Debug`. Use HTTPS for remote endpoints.

The store supports streamed reads, validated `Content-Range`, metadata, deletes,
and signed URLs. `open` prefers a signed GET URL for full reads; ranged reads stream.
Signed URLs are bearer capabilities: do not log them. Low-level signed PUT URLs
bypass adapter size validation and require separate application upload policy.

Writes use bounded disk staging followed by a single S3 PUT, with a maximum of
5 GiB; `with_max_put_bytes` can lower that bound. The coordinator assembles staged
parts before this final PUT; it does **not** use native S3 multipart uploads.
`MultipartBlobStore` is an extension trait, not an automatically selected capability.
Listing returns at most one page (default/maximum 1000 entries), not a full bucket
inventory. Providers must implement the required S3 operations; RustFS validation
does not certify every provider's failover, retention or durability settings.

## Upgrade from the earlier implementation

- Default object keys are now stable `v2/<hex-encoded-tenant>/<blob-id>`, replacing
  date-based keys. Existing objects are not deleted or automatically migrated.
  Before switching, migrate objects using their persisted receipt keys, or implement
  a stable custom key strategy backed by your historical key mapping. Never derive
  an old object's month from the current date. Custom strategies must preserve tenant
  isolation; implement `tenant_prefix` to enable safe adapter listing.
- Drain old multipart sessions before upgrading. Session records now require revision,
  expiry, final key and immutable part storage keys; old serialized sessions cannot
  be transparently resumed. Implement the new CAS contract for custom session stores.
- Add `..BlobConfig::default()` to configuration literals and configure session bounds.
- Enable `s3` explicitly if importing the built-in S3 types.
- Replace JSON file paths with trusted file handles or byte streams.
- Nonempty `key_hints` and `idempotency_key` now return an error instead of silently
  pretending to work. Implement application-level request deduplication using saved
  receipts; multipart completion and exact chunk retries are separately idempotent.

## Validation

```sh
cargo test -p dog-blob --all-features --locked
cargo clippy -p dog-blob --all-features --all-targets --locked -- -D warnings
```

The ignored S3 test only accepts a loopback RustFS endpoint and fixed dummy credentials.
It creates a unique bucket and deletes its objects and bucket after success:

```sh
docker run -d --name dogrs-blob-test -p 127.0.0.1:9000:9000 \
  -e RUSTFS_ACCESS_KEY=dogrs-local-test -e RUSTFS_SECRET_KEY=dogrs-local-test-only \
  rustfs/rustfs@sha256:8cc9801755448b71a786705ce76692c77e14936cccd87cf2fc31842e58f4d1ff /data
DOGRS_BLOB_TEST_ENDPOINT=http://127.0.0.1:9000 \
  cargo test -p dog-blob --features s3 --test s3_live --locked -- --ignored
docker rm -fv dogrs-blob-test
```

The test covers a 32 MiB unknown-length stream, ranged reads, metadata, signed reads,
tenant listing, empty objects, size rejection and deletion. CI also runs ownership,
concurrency, cancellation and integrity regression tests without external services.

MIT OR Apache-2.0.
