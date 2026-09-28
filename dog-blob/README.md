# dog-blob

Tenant-scoped object storage with pluggable backends, bounded uploads, range reads,
and resumable multipart coordination. No web server or database is required by the
core crate. The built-in S3-compatible store is enabled with `features = ["s3"]`;
default builds do not depend on the AWS SDK. The core requires Rust 1.89 or newer
for OS file locking; the currently locked S3 SDK requires Rust 1.94.1 or newer.

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

A `ValidatedUpload` owns the private staging file, size and SHA-256. The adapter
passes that same value through `BlobStore::put_validated`; S3 reads it directly.
A normal upload, including native multipart, requires **one local copy**. Explicit
resumable completion assembles one local file from remote parts. Chunk uploads
retain their individual local files until assembly succeeds, so that path can still
need roughly twice the object size. Custom backends should override `put_validated`
if their ordinary `put` implementation would otherwise stage the stream again.

DogRS enforces admission, staging-byte, total-deadline and stream-idle limits.
Defaults are 8 simultaneous upload operations, 20 GiB of staged data, a 15-minute
operation deadline and 30 seconds without a new input chunk. Excess work fails
immediately with `ResourceLimit`; timeouts return `Timeout`. These limits cover
uploads/part acceptance/completion, not every read or administrative operation.

Clone a resource handle to share a budget across components. Separate handles and
separate processes have separate budgets; filesystem quotas are still useful for
host-wide enforcement. The staging directory must exist and be trusted server
configuration. Budgets count staged bytes, not filesystem block/metadata overhead.

```rust
use dog_blob::{BlobAdapter, BlobConfig, BlobState, BlobStore, UploadLimits, UploadResources};
use std::{sync::Arc, time::Duration};
fn bounded_adapter(store: impl BlobStore + 'static) -> dog_blob::BlobResult<BlobAdapter> {
    let resources = UploadResources::new(UploadLimits {
        max_concurrent_uploads: 4,
        max_staging_bytes: 512 * 1024 * 1024,
        upload_timeout: Duration::from_secs(120),
        idle_timeout: Duration::from_secs(10),
        ..Default::default()
    })?;
    let state = BlobState::new(store, BlobConfig::default()).with_resources(resources);
    Ok(BlobAdapter::new(Arc::new(state)))
}
```

Cancellation releases local files, permits and reservations. To remove abandoned
files after process crashes, call `UploadResources::cleanup_staging` on startup or
periodically. OS locks protect live writers, including other processes using the
same local staging directory. Only DogRS staging directories are swept; unrelated
files are preserved. Tiny unpublished `.dogrs-building-*` directories left by a
crash during initial setup are deliberately not swept online. The lock/fsync
implementations are tested on Linux and macOS local filesystems, not network mounts.

`extract_file_data` keeps its legacy 8 MiB base64 limit. `put_from_multipart` uses
`BlobConfig::max_base64_bytes` (default 8 MiB, configurable up to 64 MiB), plus the
blob limit and, for chunks, the part limit. These APIs reject JSON `temp_path`
objects. Streaming is the primary large-upload API: pass a `ByteStream` to `put`, or
an already trusted open `tokio::fs::File` to `put_file`. Neither accepts permission
from a client-supplied path, and the caller's original file is never deleted.

## Native multipart and pending-write recovery

When a backend exposes `NativeMultipartStore`, uploads above
`multipart_threshold_bytes` use provider multipart operations. The S3 implementation
sends bounded ranges of the same validated file, then completes the provider upload.
The input is still fully staged/validated first; this is not a zero-disk upload path.
Other backends keep the streaming fallback and do not need any S3 types.

Pending intentions are journaled before provider initiation, and the provider handle
is saved before parts are sent. Success retains a journal entry: persist the returned
receipt, then call `acknowledge_write` with its `recovery_id`. This closes the window
where storage succeeded but the application did not receive/save the receipt.

`BlobState::with_journal` accepts any `UploadJournal`. The default memory journal
holds at most 1024 pending records and does not survive restart. For a local durable
journal, supply `FileUploadJournal` pointing to a dedicated application-owned directory:

```rust
use dog_blob::{BlobState, BlobConfig, BlobStore, FileUploadJournal};
use std::{path::Path, sync::Arc};
fn durable_state(store: impl BlobStore + 'static, directory: &Path) -> dog_blob::BlobResult<BlobState> {
    Ok(BlobState::new(store, BlobConfig::default())
        .with_journal(Arc::new(FileUploadJournal::new(directory)?)))
}
```

File records use atomic rename, fsync and OS locking. The directory must be shared
by all processes responsible for those writes; for multiple hosts, implement the
async journal lease contract using your chosen durable service. File journal I/O
runs on blocking workers, retaining its OS lock through canceled writes. Place the
directory on suitable local storage. Zero-byte lock files remain after acknowledgement to prevent inode
replacement races. Archive that directory only while its users are stopped.

Administrative `pending_writes` discovers records. `reconcile_write` acquires a
lease, skips a live writer, identifies committed objects by write identity, and
aborts only the exact abandoned native handle. It never deletes final objects.
Persist the recovery report before acknowledging its record. These are trusted
administrative APIs, not cross-tenant public endpoints.

An initiation response can be lost before its provider handle is saved. That record
stays `Uncertain`: DogRS will not guess which provider upload to abort. Provider
inventory/lifecycle handling is still required for that narrow window. Backend
errors and mismatched identities likewise preserve the record for investigation.
Non-native fallback writes do not currently use this native-write journal.

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
records retain capacity until explicitly forgotten. `recover_upload` resumes a frozen completion or aborts an expired active session.
After a session is terminal, `reconcile_staging` removes a page of its staging
objects, including unreferenced parts. Repeat until zero and again later for writes
that were already in flight. Active/Completing sessions and final objects are
protected; custom key strategies must provide an exclusive `staging_prefix`.
Do not forget session metadata until application receipts and cleanup are resolved.

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

Below the adapter threshold, S3 uses a single PUT. Above it, the adapter selects
native multipart. The current store byte limit remains 5 GiB; `with_max_put_bytes`
can lower it. Explicit resumable coordinator completion reuses one validated file
for its final PUT. `MultipartBlobStore` remains a legacy extension trait; new
capabilities implement `NativeMultipartStore`.
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
- Add `..BlobConfig::default()` to configuration literals and configure session/resource bounds.
- `put` now selects native backend multipart directly rather than creating an
  automatic coordinator session for an already complete local stream. Explicit
  `begin_multipart`/part/complete APIs remain available for resumable clients.
- Handle `ResourceLimit`/`Timeout`, and persist/acknowledge native `recovery_id` values.
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

The test covers a 32 MiB unknown-length native multipart upload with exactly one
staging file, recovery/acknowledgement, abandoned-upload abort, ranged and signed
reads, metadata, tenant listing, empty objects, size rejection and deletion. CI also runs ownership,
concurrency, cancellation and integrity regression tests without external services.

MIT OR Apache-2.0.
